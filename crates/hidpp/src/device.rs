use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::{encode_request, Error, Message, Result, SW_ID};

/// Raw report I/O. Implemented over hidapi in the app, and by fakes in tests.
pub trait Transport {
    fn write(&self, report: &[u8]) -> Result<()>;
    /// Read one input report. `Ok(0)` means the timeout elapsed with nothing to read.
    fn read(&self, buf: &mut [u8], timeout: Duration) -> Result<usize>;
}

/// One HID++ 2.0 endpoint (a device index on a transport).
///
/// Requests are synchronous. Any notification that arrives while we wait for a
/// reply is queued rather than dropped, so a button press during a settings
/// write is never lost — callers drain them with [`Device::take_notifications`].
pub struct Device<'t, T: Transport> {
    transport: &'t T,
    pub index: u8,
    features: HashMap<u16, Option<u8>>,
    notifications: VecDeque<Message>,
    pub timeout: Duration,
}

impl<'t, T: Transport> Device<'t, T> {
    pub fn new(transport: &'t T, index: u8) -> Self {
        Device { transport, index, features: HashMap::new(), notifications: VecDeque::new(), timeout: Duration::from_millis(600) }
    }

    /// Re-attach cached feature indexes (they never change for a given firmware).
    pub fn with_feature_cache(mut self, cache: HashMap<u16, Option<u8>>) -> Self {
        self.features = cache;
        self
    }

    pub fn feature_cache(&self) -> &HashMap<u16, Option<u8>> {
        &self.features
    }

    pub fn forget_features(&mut self) {
        self.features.clear();
    }

    pub fn take_notifications(&mut self) -> Vec<Message> {
        self.notifications.drain(..).collect()
    }

    /// Feed a message read by the caller's own poll loop into the notification queue.
    pub fn push_notification(&mut self, m: Message) {
        self.notifications.push_back(m);
    }

    /// Low level request on a feature index.
    pub fn call(&mut self, feature_index: u8, function: u8, params: &[u8]) -> Result<Message> {
        let req = encode_request(self.index, feature_index, function, SW_ID, params);
        self.transport.write(&req)?;
        let deadline = Instant::now() + self.timeout;
        let mut buf = [0u8; 64];
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout);
            }
            let n = self.transport.read(&mut buf, deadline - now)?;
            if n == 0 {
                continue;
            }
            let Some(m) = Message::parse(&buf[..n]) else { continue };
            if m.device_index != self.index {
                // Another endpoint on the same receiver; not ours to keep.
                continue;
            }
            if let Some((orig_feature, orig_fn_sw, code)) = m.as_error() {
                if orig_feature == feature_index && orig_fn_sw == req[3] {
                    return Err(Error::Protocol(code));
                }
                continue;
            }
            if let Some((orig_sub, code)) = m.as_legacy_error() {
                if orig_sub == feature_index {
                    return Err(Error::Legacy(code));
                }
                continue;
            }
            if m.feature_index == feature_index && m.function == function && m.sw_id == SW_ID {
                return Ok(m);
            }
            if m.is_notification() {
                self.notifications.push_back(m);
            }
        }
    }

    /// Fire-and-forget request (for commands after which the device goes away).
    pub fn send(&mut self, feature_index: u8, function: u8, params: &[u8]) -> Result<()> {
        let req = encode_request(self.index, feature_index, function, SW_ID, params);
        self.transport.write(&req)
    }

    /// Request with retry for transient failures (busy device, lost packet).
    pub fn call_retry(&mut self, feature_index: u8, function: u8, params: &[u8], attempts: u32) -> Result<Message> {
        let mut delay = Duration::from_millis(80);
        let mut last = Error::Timeout;
        for attempt in 0..attempts.max(1) {
            match self.call(feature_index, function, params) {
                Ok(m) => return Ok(m),
                Err(Error::Protocol(c)) if c.is_transient() => last = Error::Protocol(c),
                Err(Error::Timeout) => last = Error::Timeout,
                Err(e) => return Err(e),
            }
            if attempt + 1 < attempts {
                std::thread::sleep(delay);
                delay *= 3;
            }
        }
        Err(last)
    }

    /// Resolve a feature id to its index (cached). `Ok(None)` = not supported.
    pub fn feature_index(&mut self, feature: u16) -> Result<Option<u8>> {
        if feature == crate::features::ROOT {
            return Ok(Some(0));
        }
        if let Some(cached) = self.features.get(&feature) {
            return Ok(*cached);
        }
        let [hi, lo] = feature.to_be_bytes();
        let m = self.call_retry(0x00, 0x0, &[hi, lo], 3)?;
        let idx = m.u8_at(0);
        let found = if idx == 0 { None } else { Some(idx) };
        self.features.insert(feature, found);
        Ok(found)
    }

    pub fn has(&mut self, feature: u16) -> bool {
        matches!(self.feature_index(feature), Ok(Some(_)))
    }

    /// Call a function on a feature by id.
    pub fn feature_call(&mut self, feature: u16, function: u8, params: &[u8]) -> Result<Message> {
        let idx = self.feature_index(feature)?.ok_or(Error::Unsupported(feature))?;
        self.call_retry(idx, function, params, 3)
    }

    /// Root.getProtocolVersion — doubles as a ping. Returns (major, minor).
    pub fn ping(&mut self) -> Result<(u8, u8)> {
        let m = self.call(0x00, 0x1, &[0, 0, 0x5A])?;
        Ok((m.u8_at(0), m.u8_at(1)))
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::cell::RefCell;

    /// Scripted device: answers requests via a closure, can inject notifications.
    pub struct FakeTransport {
        pub inbox: RefCell<VecDeque<Vec<u8>>>,
        pub sent: RefCell<Vec<Vec<u8>>>,
        #[allow(clippy::type_complexity)]
        pub responder: Box<dyn Fn(&[u8]) -> Vec<Vec<u8>>>,
    }

    impl FakeTransport {
        pub fn new(responder: impl Fn(&[u8]) -> Vec<Vec<u8>> + 'static) -> Self {
            FakeTransport { inbox: RefCell::new(VecDeque::new()), sent: RefCell::new(vec![]), responder: Box::new(responder) }
        }
    }

    impl Transport for FakeTransport {
        fn write(&self, report: &[u8]) -> Result<()> {
            self.sent.borrow_mut().push(report.to_vec());
            for r in (self.responder)(report) {
                self.inbox.borrow_mut().push_back(r);
            }
            Ok(())
        }
        fn read(&self, buf: &mut [u8], _timeout: Duration) -> Result<usize> {
            match self.inbox.borrow_mut().pop_front() {
                Some(r) => {
                    buf[..r.len()].copy_from_slice(&r);
                    Ok(r.len())
                }
                None => Err(Error::Timeout),
            }
        }
    }

    pub fn reply(req: &[u8], params: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 20];
        out[..4].copy_from_slice(&req[..4]);
        out[4..4 + params.len()].copy_from_slice(params);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    #[test]
    fn queues_notifications_seen_while_waiting() {
        let t = FakeTransport::new(|req| {
            // A diverted-button notification arrives before the actual reply.
            let mut note = vec![0u8; 20];
            note[..6].copy_from_slice(&[0x11, 0xFF, 0x07, 0x00, 0x00, 0xC3]);
            vec![note, reply(req, &[0x07, 0x00, 0x04])]
        });
        let mut d = Device::new(&t, 0xFF);
        assert_eq!(d.feature_index(0x1B04).unwrap(), Some(7));
        let notes = d.take_notifications();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].u16_at(0), 0x00C3);
        // Cached: no second request.
        assert_eq!(d.feature_index(0x1B04).unwrap(), Some(7));
        assert_eq!(t.sent.borrow().len(), 1);
    }

    #[test]
    fn maps_error_replies() {
        let t = FakeTransport::new(|req| {
            let mut e = vec![0u8; 20];
            e[..7].copy_from_slice(&[0x11, 0xFF, 0xFF, req[2], req[3], 0x02, 0]);
            vec![e]
        });
        let mut d = Device::new(&t, 0xFF);
        assert!(matches!(d.call(0x03, 0x2, &[]), Err(Error::Protocol(crate::ErrorCode(2)))));
    }
}
