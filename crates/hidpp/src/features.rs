//! Typed wrappers for the HID++ 2.0 features Keymodus uses.

use crate::{Device, Error, Message, Result, Transport};

pub const ROOT: u16 = 0x0000;
pub const FEATURE_SET: u16 = 0x0001;
pub const DEVICE_INFO: u16 = 0x0003;
pub const DEVICE_NAME: u16 = 0x0005;
pub const BATTERY_STATUS: u16 = 0x1000;
pub const UNIFIED_BATTERY: u16 = 0x1004;
pub const CHANGE_HOST: u16 = 0x1814;
pub const REPROG_CONTROLS_V4: u16 = 0x1B04;
pub const WIRELESS_STATUS: u16 = 0x1D4B;
pub const SMART_SHIFT: u16 = 0x2110;
pub const HIRES_WHEEL: u16 = 0x2121;
pub const ADJUSTABLE_DPI: u16 = 0x2201;
pub const FN_INVERSION_MULTI_HOST: u16 = 0x40A3;
pub const BACKLIGHT2: u16 = 0x1982;
pub const DISABLE_KEYS: u16 = 0x4521;

pub fn feature_name(id: u16) -> &'static str {
    match id {
        0x0000 => "Root",
        0x0001 => "Feature set",
        0x0002 => "Feature info",
        0x0003 => "Device information",
        0x0005 => "Device name & type",
        0x0007 => "Friendly name",
        0x0020 => "Config change",
        0x0021 => "Unique random id",
        0x00C3 => "Secure DFU control",
        0x1602 => "Password authentication",
        0x1802 => "Device reset",
        0x1803 => "GPIO access",
        0x1805 => "OOB state",
        0x1806 => "Configurable device properties",
        0x1816 => "BLE Pro pre-pairing",
        0x1830 => "Power modes",
        0x1861 => "Battery levels calibration",
        0x1891 => "RF test",
        0x18A1 => "LED test",
        0x18B0 => "Monitor mode",
        0x18B1 => "Monitor mode",
        0x1A20 => "Ambient light sensor",
        0x1E00 => "Enable hidden features",
        0x1E02 => "Manage deactivatable features",
        0x1E22 => "SPI direct access",
        0x1EB0 => "TDE access",
        0x920A => "Sensor test",
        0x00C2 => "DFU control",
        0x00D0 => "DFU",
        0x1000 => "Battery status",
        0x1004 => "Unified battery",
        0x1814 => "Change host",
        0x1815 => "Hosts info",
        0x1982 => "Backlight",
        0x1B04 => "Reprogrammable controls",
        0x1D4B => "Wireless status",
        0x2110 => "SmartShift",
        0x2111 => "SmartShift (enhanced)",
        0x2121 => "Hi-res wheel",
        0x2130 => "Ratchet wheel",
        0x2150 => "Thumbwheel",
        0x2201 => "Adjustable DPI",
        0x2250 => "Analysis mode",
        0x40A3 => "Fn inversion",
        0x4220 => "Lock key state",
        0x4521 => "Disable keys",
        0x4531 => "Multiplatform",
        0x4540 => "Keyboard layout",
        0x6501 => "Gestures",
        _ => "",
    }
}

/// Every feature the device implements, as (id, version).
pub fn list_features<T: Transport>(d: &mut Device<T>) -> Result<Vec<(u16, u8)>> {
    let count = d.feature_call(FEATURE_SET, 0x0, &[])?.u8_at(0);
    let mut out = vec![(ROOT, 0)];
    for i in 1..=count {
        let m = d.feature_call(FEATURE_SET, 0x1, &[i])?;
        out.push((m.u16_at(0), m.u8_at(3)));
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Remote,
    Numpad,
    Mouse,
    Trackpad,
    Trackball,
    Presenter,
    Receiver,
    Other,
}

impl DeviceKind {
    pub fn from_code(c: u8) -> Self {
        match c {
            0 => Self::Keyboard,
            1 => Self::Remote,
            2 => Self::Numpad,
            3 => Self::Mouse,
            4 => Self::Trackpad,
            5 => Self::Trackball,
            6 => Self::Presenter,
            7 => Self::Receiver,
            _ => Self::Other,
        }
    }
    pub fn is_pointer(self) -> bool {
        matches!(self, Self::Mouse | Self::Trackpad | Self::Trackball)
    }
}

pub fn device_name<T: Transport>(d: &mut Device<T>) -> Result<String> {
    let len = d.feature_call(DEVICE_NAME, 0x0, &[])?.u8_at(0) as usize;
    let mut bytes = Vec::with_capacity(len);
    while bytes.len() < len {
        let m = d.feature_call(DEVICE_NAME, 0x1, &[bytes.len() as u8])?;
        let take = (len - bytes.len()).min(m.params.len());
        if take == 0 {
            break;
        }
        bytes.extend_from_slice(&m.params[..take]);
    }
    let s = String::from_utf8_lossy(&bytes);
    Ok(s.trim_end_matches('\0').trim().to_string())
}

pub fn device_kind<T: Transport>(d: &mut Device<T>) -> Result<DeviceKind> {
    Ok(DeviceKind::from_code(d.feature_call(DEVICE_NAME, 0x2, &[])?.u8_at(0)))
}

#[derive(Debug, Clone, Default)]
pub struct DeviceInfo {
    pub unit_id: String,
    pub model_ids: Vec<u16>,
    /// Colour / variant id, used to pick the right artwork.
    pub ext_model: u8,
    pub firmware: Vec<String>,
}

pub fn device_info<T: Transport>(d: &mut Device<T>) -> Result<DeviceInfo> {
    let m = d.feature_call(DEVICE_INFO, 0x0, &[])?;
    let entities = m.u8_at(0);
    let unit_id = m.params.get(1..5).map(hex).unwrap_or_default();
    let model_ids = (0..3).map(|i| m.u16_at(7 + i * 2)).filter(|&p| p != 0).collect();
    let mut firmware = vec![];
    for e in 0..entities.min(4) {
        let Ok(f) = d.feature_call(DEVICE_INFO, 0x1, &[e]) else { continue };
        // type 0 = main application firmware; 1 = bootloader; 2 = hardware; 5 = radio
        let kind = match f.u8_at(0) {
            0 => "Firmware",
            1 => "Bootloader",
            2 => "Hardware",
            5 => "Radio",
            _ => continue,
        };
        let prefix: String = f.params[1..4].iter().filter(|b| b.is_ascii_graphic()).map(|&b| b as char).collect();
        firmware.push(format!("{kind}: {prefix} {:02x}.{:02x}.B{:04x}", f.u8_at(4), f.u8_at(5), f.u16_at(6)));
    }
    let ext_model = m.u8_at(13);
    Ok(DeviceInfo { unit_id, model_ids, ext_model, firmware })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: Option<u8>,
    pub charging: bool,
    /// Coarse level when the device does not report a percentage.
    pub level: &'static str,
}

pub fn battery<T: Transport>(d: &mut Device<T>) -> Result<Battery> {
    if d.has(UNIFIED_BATTERY) {
        let m = d.feature_call(UNIFIED_BATTERY, 0x1, &[])?;
        let pct = m.u8_at(0);
        let level = match m.u8_at(1) {
            l if l & 0x08 != 0 => "full",
            l if l & 0x04 != 0 => "good",
            l if l & 0x02 != 0 => "low",
            l if l & 0x01 != 0 => "critical",
            _ => "unknown",
        };
        let charging = matches!(m.u8_at(2), 1..=3);
        return Ok(Battery { percent: (pct > 0).then_some(pct), charging, level });
    }
    if d.has(BATTERY_STATUS) {
        let m = d.feature_call(BATTERY_STATUS, 0x0, &[])?;
        let pct = m.u8_at(0);
        let charging = matches!(m.u8_at(2), 1..=3);
        return Ok(Battery { percent: (pct > 0).then_some(pct), charging, level: "unknown" });
    }
    Err(Error::Unsupported(UNIFIED_BATTERY))
}

// ---------------------------------------------------------------------------
// 0x1B04 Reprogrammable controls v4 — the heart of button remapping.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub cid: u16,
    pub tid: u16,
    pub flags: u8,
    pub position: u8,
    pub group: u8,
    pub group_mask: u8,
    pub extra_flags: u8,
}

impl Control {
    pub fn is_mouse_button(&self) -> bool {
        self.flags & 0x01 != 0
    }
    pub fn is_fkey(&self) -> bool {
        self.flags & 0x02 != 0
    }
    pub fn divertable(&self) -> bool {
        self.flags & 0x20 != 0
    }
    pub fn is_virtual(&self) -> bool {
        self.flags & 0x80 != 0
    }
    /// Supports reporting raw pointer movement while held (gestures).
    pub fn raw_xy(&self) -> bool {
        self.extra_flags & 0x01 != 0
    }
}

pub fn controls<T: Transport>(d: &mut Device<T>) -> Result<Vec<Control>> {
    let count = d.feature_call(REPROG_CONTROLS_V4, 0x0, &[])?.u8_at(0);
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let m = d.feature_call(REPROG_CONTROLS_V4, 0x1, &[i])?;
        out.push(Control {
            cid: m.u16_at(0),
            tid: m.u16_at(2),
            flags: m.u8_at(4),
            position: m.u8_at(5),
            group: m.u8_at(6),
            group_mask: m.u8_at(7),
            extra_flags: m.u8_at(8),
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reporting {
    pub diverted: bool,
    pub persist: bool,
    pub raw_xy: bool,
    pub remap: u16,
}

pub fn get_reporting<T: Transport>(d: &mut Device<T>, cid: u16) -> Result<Reporting> {
    let [hi, lo] = cid.to_be_bytes();
    let m = d.feature_call(REPROG_CONTROLS_V4, 0x2, &[hi, lo])?;
    let f = m.u8_at(2);
    Ok(Reporting { diverted: f & 0x01 != 0, persist: f & 0x04 != 0, raw_xy: f & 0x10 != 0, remap: m.u16_at(3) })
}

/// Divert (or release) a control. Diversion is volatile: the device forgets it
/// on power cycle / reconnect, which is why the engine re-applies and verifies.
pub fn set_divert<T: Transport>(d: &mut Device<T>, cid: u16, divert: bool, raw_xy: bool) -> Result<()> {
    let [hi, lo] = cid.to_be_bytes();
    // bit0 divert, bit1 divert-valid, bit4 rawXY, bit5 rawXY-valid
    let mut flags = 0x02 | 0x20;
    if divert {
        flags |= 0x01;
        if raw_xy {
            flags |= 0x10;
        }
    }
    d.feature_call(REPROG_CONTROLS_V4, 0x3, &[hi, lo, flags, 0, 0])?;
    Ok(())
}

/// Events emitted by 0x1B04 while controls are diverted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEvent {
    /// The full set of diverted controls currently held down.
    Pressed(Vec<u16>),
    /// Pointer motion while a raw-XY diverted control is held.
    RawXY { dx: i16, dy: i16 },
}

pub fn parse_control_event(m: &Message) -> Option<ControlEvent> {
    match m.function {
        0 => Some(ControlEvent::Pressed((0..4).map(|i| m.u16_at(i * 2)).filter(|&c| c != 0).collect())),
        1 => Some(ControlEvent::RawXY { dx: m.i16_at(0), dy: m.i16_at(2) }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 0x2201 Adjustable DPI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiInfo {
    pub current: u16,
    pub default: u16,
    /// Discrete supported values (ranges are expanded with their step).
    pub supported: Vec<u16>,
}

pub fn dpi<T: Transport>(d: &mut Device<T>) -> Result<DpiInfo> {
    let cur = d.feature_call(ADJUSTABLE_DPI, 0x2, &[0])?;
    let m = d.feature_call(ADJUSTABLE_DPI, 0x1, &[0])?;
    let raw: Vec<u16> = (0..7).map(|i| m.u16_at(1 + i * 2)).take_while(|&v| v != 0).collect();
    Ok(DpiInfo { current: cur.u16_at(1), default: cur.u16_at(3), supported: expand_dpi_list(&raw) })
}

/// Values with the top three bits set encode a step between their neighbours.
pub fn expand_dpi_list(raw: &[u16]) -> Vec<u16> {
    let mut out = vec![];
    let mut i = 0;
    while i < raw.len() {
        let v = raw[i];
        if v >= 0xE000 {
            let step = v & 0x1FFF;
            if let (Some(&lo), Some(&hi)) = (out.last(), raw.get(i + 1)) {
                let mut x: u16 = lo;
                while step > 0 && x + step <= hi {
                    x += step;
                    out.push(x);
                }
                if out.last() != Some(&hi) {
                    out.push(hi);
                }
                i += 2;
                continue;
            }
        } else {
            out.push(v);
        }
        i += 1;
    }
    out
}

pub fn set_dpi<T: Transport>(d: &mut Device<T>, dpi: u16) -> Result<()> {
    let [hi, lo] = dpi.to_be_bytes();
    d.feature_call(ADJUSTABLE_DPI, 0x3, &[0, hi, lo])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 0x2121 Hi-res wheel, 0x2110 SmartShift
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelMode {
    pub hires: bool,
    pub inverted: bool,
    target_hidpp: bool,
}

pub fn wheel_mode<T: Transport>(d: &mut Device<T>) -> Result<WheelMode> {
    let f = d.feature_call(HIRES_WHEEL, 0x1, &[])?.u8_at(0);
    Ok(WheelMode { target_hidpp: f & 0x01 != 0, hires: f & 0x02 != 0, inverted: f & 0x04 != 0 })
}

pub fn set_wheel_mode<T: Transport>(d: &mut Device<T>, hires: bool, inverted: bool) -> Result<()> {
    // Never divert the wheel to HID++ — that would stop native scrolling.
    let f = (hires as u8) << 1 | (inverted as u8) << 2;
    d.feature_call(HIRES_WHEEL, 0x2, &[f])?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmartShift {
    /// true = ratchet (clicky) wheel, false = free spin.
    pub ratchet: bool,
    /// Speed threshold at which the wheel auto-disengages to free spin (1..=50; 0xFF = never).
    pub threshold: u8,
}

pub fn smartshift<T: Transport>(d: &mut Device<T>) -> Result<SmartShift> {
    let m = d.feature_call(SMART_SHIFT, 0x0, &[])?;
    Ok(SmartShift { ratchet: m.u8_at(0) == 2, threshold: m.u8_at(1) })
}

pub fn set_smartshift<T: Transport>(d: &mut Device<T>, s: SmartShift) -> Result<()> {
    d.feature_call(SMART_SHIFT, 0x1, &[if s.ratchet { 2 } else { 1 }, s.threshold.max(1), 0])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 0x40A3 Fn inversion (multi-host keyboards), 0x1814 change host
// ---------------------------------------------------------------------------

/// true = F1..F12 send function keys by default; media actions need Fn.
pub fn fn_inversion<T: Transport>(d: &mut Device<T>) -> Result<bool> {
    let m = d.feature_call(FN_INVERSION_MULTI_HOST, 0x0, &[0xFF])?;
    Ok(m.u8_at(1) & 0x01 != 0)
}

pub fn set_fn_inversion<T: Transport>(d: &mut Device<T>, on: bool) -> Result<()> {
    d.feature_call(FN_INVERSION_MULTI_HOST, 0x1, &[0xFF, on as u8])?;
    Ok(())
}

/// (number of host channels, current channel 0-based)
pub fn host_info<T: Transport>(d: &mut Device<T>) -> Result<(u8, u8)> {
    let m = d.feature_call(CHANGE_HOST, 0x0, &[])?;
    Ok((m.u8_at(0), m.u8_at(1)))
}

/// Move the device to another Easy-Switch channel (0-based). The device
/// disconnects from this computer right away, so there is no reply to wait for.
pub fn set_host<T: Transport>(d: &mut Device<T>, host: u8) -> Result<()> {
    let idx = d.feature_index(CHANGE_HOST)?.ok_or(Error::Unsupported(CHANGE_HOST))?;
    d.send(idx, 0x1, &[host])
}

// ---------------------------------------------------------------------------
// 0x1982 Backlight (v2), 0x4521 Disable keys
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklightMode {
    /// Firmware default (on, driven by the device itself).
    Basic,
    /// Follows the ambient light sensor.
    Auto,
    /// Fixed level set by the host.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backlight {
    pub enabled: bool,
    pub mode: BacklightMode,
    pub level: u8,
    /// Number of manual levels (from function 2).
    pub levels: u8,
    pub auto_supported: bool,
    /// Host-set fixed level ("permanent" mode) is available.
    pub manual_supported: bool,
    options: u8,
    dho: u16,
    dhi: u16,
    dpow: u16,
}

pub fn backlight<T: Transport>(d: &mut Device<T>) -> Result<Backlight> {
    let m = d.feature_call(BACKLIGHT2, 0x0, &[])?;
    let le = |i: usize| u16::from_le_bytes([m.u8_at(i), m.u8_at(i + 1)]);
    let options = m.u8_at(1);
    let supported = m.u8_at(2);
    let levels = d.feature_call(BACKLIGHT2, 0x2, &[]).map(|r| r.u8_at(0)).unwrap_or(0);
    Ok(Backlight {
        enabled: m.u8_at(0) != 0,
        mode: match (options >> 3) & 0x03 {
            1 => BacklightMode::Auto,
            3 => BacklightMode::Manual,
            _ => BacklightMode::Basic,
        },
        level: m.u8_at(5),
        levels,
        auto_supported: supported & 0x08 != 0,
        manual_supported: supported & 0x20 != 0,
        options,
        dho: le(6),
        dhi: le(8),
        dpow: le(10),
    })
}

/// Write enabled / mode / level, keeping the device's timeouts as they were.
pub fn set_backlight<T: Transport>(d: &mut Device<T>, cur: &Backlight, enabled: bool, mode: BacklightMode, level: u8) -> Result<()> {
    let mode_bits = match mode {
        BacklightMode::Basic => 0,
        BacklightMode::Auto => 1,
        BacklightMode::Manual => 3,
    };
    let options = (cur.options & 0x07) | (mode_bits << 3);
    let level = if mode == BacklightMode::Manual { level.min(cur.levels.saturating_sub(1)) } else { 0 };
    let mut p = vec![enabled as u8, options, 0xFF, level];
    for v in [cur.dho, cur.dhi, cur.dpow] {
        p.extend_from_slice(&v.to_le_bytes());
    }
    d.feature_call(BACKLIGHT2, 0x1, &p)?;
    Ok(())
}

/// (keys that can be disabled, keys currently disabled) as bitmasks:
/// 1 Caps Lock, 2 Num Lock, 4 Scroll Lock, 8 Insert, 16 Windows/⌘.
pub fn disabled_keys<T: Transport>(d: &mut Device<T>) -> Result<(u8, u8)> {
    let caps = d.feature_call(DISABLE_KEYS, 0x0, &[])?.u8_at(0);
    let cur = d.feature_call(DISABLE_KEYS, 0x1, &[])?.u8_at(0);
    Ok((caps, cur))
}

pub fn set_disabled_keys<T: Transport>(d: &mut Device<T>, mask: u8) -> Result<()> {
    d.feature_call(DISABLE_KEYS, 0x2, &[mask])?;
    Ok(())
}

/// 0x1D4B event: the device reconnected and asks the host to re-send its configuration.
pub fn is_reconfigure_request(m: &Message) -> bool {
    m.function == 0 && (m.u8_at(0) == 1 || m.u8_at(1) == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::fake::{reply, FakeTransport};

    #[test]
    fn expands_dpi_ranges() {
        assert_eq!(expand_dpi_list(&[400, 800, 1600]), vec![400, 800, 1600]);
        assert_eq!(expand_dpi_list(&[400, 0xE000 | 400, 1600]), vec![400, 800, 1200, 1600]);
        assert_eq!(expand_dpi_list(&[200, 0xE000 | 300, 1000]), vec![200, 500, 800, 1000]);
    }

    #[test]
    fn divert_sets_valid_bits() {
        let t = FakeTransport::new(|req| {
            if req[2] == 0 {
                vec![reply(req, &[0x09, 0, 0])] // 0x1B04 lives at index 9
            } else {
                vec![reply(req, &req[4..9])]
            }
        });
        let mut d = Device::new(&t, 0xFF);
        set_divert(&mut d, 0x00C3, true, true).unwrap();
        set_divert(&mut d, 0x0053, false, false).unwrap();
        let sent = t.sent.borrow();
        assert_eq!(&sent[1][2..7], &[0x09, 0x3B, 0x00, 0xC3, 0x33]);
        assert_eq!(&sent[2][4..7], &[0x00, 0x53, 0x22]);
    }

    /// Fake device with one feature at index 5 that echoes writes and answers reads from a table.
    fn feature_at_5(reads: Vec<(u8, Vec<u8>)>) -> FakeTransport {
        FakeTransport::new(move |req| {
            if req[2] == 0 {
                return vec![reply(req, &[0x05, 0, 0])];
            }
            let func = req[3] >> 4;
            let params = reads.iter().find(|(f, _)| *f == func).map(|(_, p)| p.clone()).unwrap_or_default();
            vec![reply(req, &params)]
        })
    }

    #[test]
    fn reads_and_writes_backlight() {
        // enabled, options 0x0D (mode 1 = auto), supported 0x0D, effects, level 3, dho 60, dhi 30, dpow 300 (LE)
        let t = feature_at_5(vec![(0, vec![1, 0x0D, 0x0D, 0x7F, 0, 3, 60, 0, 30, 0, 0x2C, 0x01]), (2, vec![8])]);
        let mut d = Device::new(&t, 0xFF);
        let b = backlight(&mut d).unwrap();
        assert!(b.enabled);
        assert_eq!(b.mode, BacklightMode::Auto);
        assert_eq!(b.levels, 8);
        assert!(b.auto_supported);
        set_backlight(&mut d, &b, true, BacklightMode::Manual, 20).unwrap();
        let sent = t.sent.borrow();
        let w = sent.last().unwrap();
        assert_eq!(w[3] >> 4, 1);
        // options keep low bits, mode 3; level clamped to 7; timeouts preserved little-endian
        assert_eq!(&w[4..14], &[1, 0x1D, 0xFF, 7, 60, 0, 30, 0, 0x2C, 0x01]);
    }

    #[test]
    fn disable_keys_masks() {
        let t = feature_at_5(vec![(0, vec![0x01]), (1, vec![0x00])]);
        let mut d = Device::new(&t, 0xFF);
        assert_eq!(disabled_keys(&mut d).unwrap(), (1, 0));
        set_disabled_keys(&mut d, 1).unwrap();
        let sent = t.sent.borrow();
        assert_eq!(&sent.last().unwrap()[2..5], &[0x05, 0x2B, 0x01]);
    }

    #[test]
    fn set_host_does_not_wait_for_reply() {
        let t = FakeTransport::new(|req| if req[2] == 0 { vec![reply(req, &[0x04, 0, 0])] } else { vec![] });
        let mut d = Device::new(&t, 0xFF);
        set_host(&mut d, 1).unwrap();
        assert_eq!(&t.sent.borrow().last().unwrap()[2..5], &[0x04, 0x1B, 0x01]);
    }

    #[test]
    fn parses_pressed_controls() {
        let raw = [0x11, 0xFF, 0x09, 0x00, 0x00, 0x53, 0x00, 0xC3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let m = Message::parse(&raw).unwrap();
        assert_eq!(parse_control_event(&m), Some(ControlEvent::Pressed(vec![0x53, 0xC3])));
        let raw = [0x11, 0xFF, 0x09, 0x10, 0xFF, 0xF6, 0x00, 0x0A, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let m = Message::parse(&raw).unwrap();
        assert_eq!(parse_control_event(&m), Some(ControlEvent::RawXY { dx: -10, dy: 10 }));
    }
}
