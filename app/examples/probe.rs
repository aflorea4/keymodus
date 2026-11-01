//! HID++ probe for contributors: lists features and reprogrammable controls of
//! every connected Logitech HID++ endpoint. Read-only by default.
//!
//! `cargo run -p keymodus --example probe`
//! `cargo run -p keymodus --example probe -- --selftest` additionally checks the
//! backlight and disable-keys writes: it writes the current backlight state back
//! unchanged and toggles "disable Caps Lock" on and off again.

use std::time::Duration;

use hidpp::{features as f, Device, Transport};

struct Hid(hidapi::HidDevice);

impl Transport for Hid {
    fn write(&self, r: &[u8]) -> hidpp::Result<()> {
        self.0.write(r).map(|_| ()).map_err(|e| hidpp::Error::Transport(e.to_string()))
    }
    fn read(&self, buf: &mut [u8], t: Duration) -> hidpp::Result<usize> {
        self.0.read_timeout(buf, t.as_millis() as i32).map_err(|e| hidpp::Error::Transport(e.to_string()))
    }
}

fn selftest(d: &mut Device<Hid>) {
    if let Ok(b) = f::backlight(d) {
        println!("  backlight before: {b:?}");
        let r = f::set_backlight(d, &b, b.enabled, b.mode, b.level);
        println!("  backlight rewrite: {r:?}; after: {:?}", f::backlight(d).map(|x| (x.enabled, x.mode, x.level)));
    }
    if let Ok((caps, cur)) = f::disabled_keys(d) {
        println!("  disable-keys caps {caps:#04x} current {cur:#04x}");
        if caps & 1 != 0 {
            let on = f::set_disabled_keys(d, cur | 1).and_then(|_| f::disabled_keys(d));
            let off = f::set_disabled_keys(d, cur).and_then(|_| f::disabled_keys(d));
            println!("  caps-lock disable on -> {on:?}; restored -> {off:?}");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let write = args.iter().any(|a| a == "--selftest");
    // --raw <pid hex> <feature hex> <function> <params hex>: one explicit write, for recovery.
    let raw_write = args.iter().position(|a| a == "--raw").map(|i| {
        let h = |s: &str| u16::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex");
        let bytes = (0..args[i + 4].len()).step_by(2).map(|j| u8::from_str_radix(&args[i + 4][j..j + 2], 16).expect("hex")).collect::<Vec<u8>>();
        (h(&args[i + 1]), h(&args[i + 2]), args[i + 3].parse::<u8>().expect("function"), bytes)
    });
    let api = hidapi::HidApi::new().expect("hidapi");
    for info in api.device_list() {
        let bt = info.usage_page() == 0xFF43;
        let usb = info.usage_page() == 0xFF00 && info.usage() == 2;
        if info.vendor_id() != hidpp::LOGITECH_VID || !(bt || usb) {
            continue;
        }
        println!("== {:04x}:{:04x} {}", info.vendor_id(), info.product_id(), info.product_string().unwrap_or(""));
        let Ok(dev) = info.open_device(&api) else {
            println!("  (cannot open)");
            continue;
        };
        let t = Hid(dev);
        let mut d = Device::new(&t, hidpp::DIRECT);
        match f::list_features(&mut d) {
            Ok(list) => {
                for (id, v) in list {
                    println!("  0x{id:04X} v{v} {}", f::feature_name(id));
                }
            }
            Err(e) => println!("  features: {e}"),
        }
        if let Ok(ctrls) = f::controls(&mut d) {
            for c in ctrls {
                println!(
                    "  cid 0x{:04X} tid 0x{:04X} flags 0x{:02X} pos {} group {} mask 0x{:02X} extra 0x{:02X}",
                    c.cid, c.tid, c.flags, c.position, c.group, c.group_mask, c.extra_flags
                );
            }
        }
        if write {
            selftest(&mut d);
        }
        // Getter functions only: on many features function 1 is a setter, and
        // calling it with no parameters would reset the device's settings.
        const GETTERS: &[(u16, u8)] = &[(0x1982, 0), (0x1982, 2), (0x4521, 0), (0x4521, 1), (0x1815, 0)];
        for &(feat, func) in GETTERS {
            if let Ok(Some(_)) = d.feature_index(feat) {
                if let Ok(m) = d.feature_call(feat, func, &[]) {
                    println!("  0x{feat:04X} fn{func}: {:02X?}", &m.params[..16.min(m.params.len())]);
                }
            }
        }
        if let Some(raw) = &raw_write {
            if raw.0 == info.product_id() {
                let r = d.feature_call(raw.1, raw.2, &raw.3);
                println!("  raw write 0x{:04X} fn{} {:02X?} -> {:?}", raw.1, raw.2, raw.3, r.map(|m| m.params[..8].to_vec()));
            }
        }
    }
}
