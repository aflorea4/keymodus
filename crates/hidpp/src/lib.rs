//! Logitech HID++ 2.0.
//!
//! Every HID++ 2.0 message is a long (20 byte) report:
//!
//! ```text
//! [0x11, device_index, feature_index, (function << 4) | sw_id, params...16]
//! ```
//!
//! Devices connected directly (Bluetooth, USB cable) answer on device index
//! `0xFF`; devices behind a Unifying / Bolt receiver answer on 1..=6.
//! Responses echo `feature_index` and `function|sw_id`. Notifications (button
//! events, connection changes) carry `sw_id == 0`, which is how we tell them
//! apart from replies to our own requests.

mod device;
pub mod features;

pub use device::{Device, Transport};

use std::fmt;

pub const REPORT_SHORT: u8 = 0x10;
pub const REPORT_LONG: u8 = 0x11;
pub const REPORT_VERY_LONG: u8 = 0x12;
pub const LONG_LEN: usize = 20;

/// Device index used by devices that are not behind a receiver.
pub const DIRECT: u8 = 0xFF;

/// Our software id. Anything non-zero works; zero is reserved for notifications.
pub const SW_ID: u8 = 0x0B;

pub const LOGITECH_VID: u16 = 0x046D;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("transport: {0}")]
    Transport(String),
    #[error("device did not answer in time")]
    Timeout,
    #[error("device returned HID++ 2.0 error {0}")]
    Protocol(ErrorCode),
    #[error("device returned HID++ 1.0 error 0x{0:02x}")]
    Legacy(u8),
    #[error("feature 0x{0:04x} not supported by this device")]
    Unsupported(u16),
    #[error("malformed reply")]
    Malformed,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCode(pub u8);

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            0 => "no error",
            1 => "unknown",
            2 => "invalid argument",
            3 => "out of range",
            4 => "hardware error",
            5 => "logitech internal",
            6 => "invalid feature index",
            7 => "invalid function id",
            8 => "busy",
            9 => "unsupported",
            _ => "other",
        };
        write!(f, "0x{:02x} ({name})", self.0)
    }
}

impl ErrorCode {
    /// Errors worth retrying: the device is alive but momentarily unable to serve us.
    pub fn is_transient(self) -> bool {
        matches!(self.0, 1 | 4 | 8)
    }
}

/// A decoded inbound HID++ report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub report_id: u8,
    pub device_index: u8,
    pub feature_index: u8,
    pub function: u8,
    pub sw_id: u8,
    pub params: Vec<u8>,
}

impl Message {
    pub fn parse(raw: &[u8]) -> Option<Message> {
        if raw.len() < 4 {
            return None;
        }
        let report_id = raw[0];
        if !matches!(report_id, REPORT_SHORT | REPORT_LONG | REPORT_VERY_LONG) {
            return None;
        }
        Some(Message {
            report_id,
            device_index: raw[1],
            feature_index: raw[2],
            function: raw[3] >> 4,
            sw_id: raw[3] & 0x0F,
            params: raw[4..].to_vec(),
        })
    }

    pub fn is_notification(&self) -> bool {
        self.sw_id == 0
    }

    /// HID++ 2.0 error reply: feature index 0xFF, params = [orig_feature, orig_fn|sw, code].
    pub fn as_error(&self) -> Option<(u8, u8, ErrorCode)> {
        if self.feature_index == 0xFF && self.params.len() >= 2 {
            let orig_fn_sw = self.params[0];
            let code = self.params[1];
            return Some((self.function_byte_raw(), orig_fn_sw, ErrorCode(code)));
        }
        None
    }

    /// HID++ 1.0 error: report 0x10, sub id 0x8F, params = [orig_sub, orig_addr, code].
    pub fn as_legacy_error(&self) -> Option<(u8, u8)> {
        if self.report_id == REPORT_SHORT && self.feature_index == 0x8F && self.params.len() >= 2 {
            return Some((self.function_byte_raw(), self.params[1]));
        }
        None
    }

    fn function_byte_raw(&self) -> u8 {
        (self.function << 4) | self.sw_id
    }

    pub fn u8_at(&self, i: usize) -> u8 {
        self.params.get(i).copied().unwrap_or(0)
    }

    pub fn u16_at(&self, i: usize) -> u16 {
        u16::from_be_bytes([self.u8_at(i), self.u8_at(i + 1)])
    }

    pub fn i16_at(&self, i: usize) -> i16 {
        self.u16_at(i) as i16
    }
}

/// Encode a long HID++ 2.0 request.
pub fn encode_request(device_index: u8, feature_index: u8, function: u8, sw_id: u8, params: &[u8]) -> [u8; LONG_LEN] {
    let mut out = [0u8; LONG_LEN];
    out[0] = REPORT_LONG;
    out[1] = device_index;
    out[2] = feature_index;
    out[3] = ((function & 0x0F) << 4) | (sw_id & 0x0F);
    let n = params.len().min(LONG_LEN - 4);
    out[4..4 + n].copy_from_slice(&params[..n]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_long_request() {
        let r = encode_request(DIRECT, 0x00, 0x1, SW_ID, &[0, 0, 0x5A]);
        assert_eq!(&r[..7], &[0x11, 0xFF, 0x00, 0x1B, 0x00, 0x00, 0x5A]);
        assert_eq!(r.len(), 20);
    }

    #[test]
    fn parses_error_reply() {
        let raw = [0x11, 0xFF, 0xFF, 0x05, 0x3B, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let m = Message::parse(&raw).unwrap();
        let (_, fn_sw, code) = m.as_error().unwrap();
        assert_eq!(fn_sw, 0x3B);
        assert_eq!(code, ErrorCode(8));
        assert!(code.is_transient());
    }

    #[test]
    fn distinguishes_notifications() {
        let raw = [0x11, 0xFF, 0x06, 0x00, 0x00, 0x53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let m = Message::parse(&raw).unwrap();
        assert!(m.is_notification());
        assert_eq!(m.u16_at(0), 0x0053);
    }
}
