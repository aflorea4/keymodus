//! Key naming. Keys are stored as `KeyboardEvent.code` strings so the UI's
//! recorder, the config file and each platform's synthesizer share one vocabulary.

/// USB HID keyboard-page usage -> `KeyboardEvent.code`.
pub fn hid_usage_to_code(u: u8) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ",
        "KeyR", "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ",
    ];
    const DIGITS: [&str; 10] = ["Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8", "Digit9", "Digit0"];
    const F: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];
    const F13: [&str; 8] = ["F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20"];
    Some(match u {
        4..=29 => LETTERS[(u - 4) as usize],
        30..=39 => DIGITS[(u - 30) as usize],
        40 => "Enter",
        41 => "Escape",
        42 => "Backspace",
        43 => "Tab",
        44 => "Space",
        45 => "Minus",
        46 => "Equal",
        47 => "BracketLeft",
        48 => "BracketRight",
        49 => "Backslash",
        51 => "Semicolon",
        52 => "Quote",
        53 => "Backquote",
        54 => "Comma",
        55 => "Period",
        56 => "Slash",
        58..=69 => F[(u - 58) as usize],
        74 => "Home",
        75 => "PageUp",
        76 => "Delete",
        77 => "End",
        78 => "PageDown",
        79 => "ArrowRight",
        80 => "ArrowLeft",
        81 => "ArrowDown",
        82 => "ArrowUp",
        104..=111 => F13[(u - 104) as usize],
        _ => return None,
    })
}

/// One row per key: (`KeyboardEvent.code`, macOS kVK, Windows VK, Linux evdev KEY_*).
/// `0xFFFF` = no such key on that platform.
const NONE: u16 = 0xFFFF;
#[rustfmt::skip]
const KEYS: &[(&str, u16, u16, u16)] = &[
    ("KeyA", 0x00, 0x41, 30), ("KeyB", 0x0B, 0x42, 48), ("KeyC", 0x08, 0x43, 46), ("KeyD", 0x02, 0x44, 32),
    ("KeyE", 0x0E, 0x45, 18), ("KeyF", 0x03, 0x46, 33), ("KeyG", 0x05, 0x47, 34), ("KeyH", 0x04, 0x48, 35),
    ("KeyI", 0x22, 0x49, 23), ("KeyJ", 0x26, 0x4A, 36), ("KeyK", 0x28, 0x4B, 37), ("KeyL", 0x25, 0x4C, 38),
    ("KeyM", 0x2E, 0x4D, 50), ("KeyN", 0x2D, 0x4E, 49), ("KeyO", 0x1F, 0x4F, 24), ("KeyP", 0x23, 0x50, 25),
    ("KeyQ", 0x0C, 0x51, 16), ("KeyR", 0x0F, 0x52, 19), ("KeyS", 0x01, 0x53, 31), ("KeyT", 0x11, 0x54, 20),
    ("KeyU", 0x20, 0x55, 22), ("KeyV", 0x09, 0x56, 47), ("KeyW", 0x0D, 0x57, 17), ("KeyX", 0x07, 0x58, 45),
    ("KeyY", 0x10, 0x59, 21), ("KeyZ", 0x06, 0x5A, 44),
    ("Digit0", 0x1D, 0x30, 11), ("Digit1", 0x12, 0x31, 2), ("Digit2", 0x13, 0x32, 3), ("Digit3", 0x14, 0x33, 4),
    ("Digit4", 0x15, 0x34, 5), ("Digit5", 0x17, 0x35, 6), ("Digit6", 0x16, 0x36, 7), ("Digit7", 0x1A, 0x37, 8),
    ("Digit8", 0x1C, 0x38, 9), ("Digit9", 0x19, 0x39, 10),
    ("Enter", 0x24, 0x0D, 28), ("Escape", 0x35, 0x1B, 1), ("Backspace", 0x33, 0x08, 14), ("Tab", 0x30, 0x09, 15),
    ("Space", 0x31, 0x20, 57), ("Minus", 0x1B, 0xBD, 12), ("Equal", 0x18, 0xBB, 13), ("BracketLeft", 0x21, 0xDB, 26),
    ("BracketRight", 0x1E, 0xDD, 27), ("Backslash", 0x2A, 0xDC, 43), ("Semicolon", 0x29, 0xBA, 39), ("Quote", 0x27, 0xDE, 40),
    ("Backquote", 0x32, 0xC0, 41), ("Comma", 0x2B, 0xBC, 51), ("Period", 0x2F, 0xBE, 52), ("Slash", 0x2C, 0xBF, 53),
    ("F1", 0x7A, 0x70, 59), ("F2", 0x78, 0x71, 60), ("F3", 0x63, 0x72, 61), ("F4", 0x76, 0x73, 62),
    ("F5", 0x60, 0x74, 63), ("F6", 0x61, 0x75, 64), ("F7", 0x62, 0x76, 65), ("F8", 0x64, 0x77, 66),
    ("F9", 0x65, 0x78, 67), ("F10", 0x6D, 0x79, 68), ("F11", 0x67, 0x7A, 87), ("F12", 0x6F, 0x7B, 88),
    ("F13", 0x69, 0x7C, 183), ("F14", 0x6B, 0x7D, 184), ("F15", 0x71, 0x7E, 185), ("F16", 0x6A, 0x7F, 186),
    ("F17", 0x40, 0x80, 187), ("F18", 0x4F, 0x81, 188), ("F19", 0x50, 0x82, 189), ("F20", 0x5A, 0x83, 190),
    ("Home", 0x73, 0x24, 102), ("End", 0x77, 0x23, 107), ("PageUp", 0x74, 0x21, 104), ("PageDown", 0x79, 0x22, 109),
    ("Delete", 0x75, 0x2E, 111), ("Insert", 0x72, 0x2D, 110), ("PrintScreen", NONE, 0x2C, 99), ("CapsLock", 0x39, 0x14, 58),
    ("ArrowLeft", 0x7B, 0x25, 105), ("ArrowRight", 0x7C, 0x27, 106), ("ArrowUp", 0x7E, 0x26, 103), ("ArrowDown", 0x7D, 0x28, 108),
];

fn lookup(code: &str, col: fn(&(&str, u16, u16, u16)) -> u16) -> Option<u16> {
    KEYS.iter().find(|k| k.0 == code).map(col).filter(|&v| v != NONE)
}

fn reverse(v: u16, col: fn(&(&str, u16, u16, u16)) -> u16) -> Option<&'static str> {
    KEYS.iter().find(|k| col(k) == v).map(|k| k.0)
}

/// `KeyboardEvent.code` -> macOS virtual key code (kVK_*).
pub fn code_to_mac_keycode(code: &str) -> Option<u16> {
    lookup(code, |k| k.1)
}

pub fn mac_keycode_to_code(kc: u16) -> Option<&'static str> {
    reverse(kc, |k| k.1)
}

/// `KeyboardEvent.code` -> Windows virtual-key code (VK_*).
pub fn code_to_windows_vk(code: &str) -> Option<u16> {
    lookup(code, |k| k.2)
}

pub fn windows_vk_to_code(vk: u16) -> Option<&'static str> {
    reverse(vk, |k| k.2)
}

/// `KeyboardEvent.code` -> Linux input event code (KEY_*).
pub fn code_to_evdev(code: &str) -> Option<u16> {
    lookup(code, |k| k.3)
}

pub fn evdev_to_code(key: u16) -> Option<&'static str> {
    reverse(key, |k| k.3)
}

/// Every key code the app understands.
pub fn all_codes() -> impl Iterator<Item = &'static str> {
    KEYS.iter().map(|k| k.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_maps_on_every_platform() {
        for c in all_codes() {
            assert!(code_to_mac_keycode(c).is_some() || c == "PrintScreen", "{c}");
            assert!(code_to_windows_vk(c).is_some(), "{c}");
            assert!(code_to_evdev(c).is_some(), "{c}");
        }
        let mut seen = std::collections::HashSet::new();
        assert!(all_codes().all(|c| seen.insert(c)), "duplicate code");
        assert_eq!(windows_vk_to_code(0x70), Some("F1"));
        assert_eq!(evdev_to_code(183), Some("F13"));
        assert_eq!(hid_usage_to_code(44), Some("Space"));
        assert_eq!(code_to_mac_keycode("Space"), Some(0x31));
        assert_eq!(mac_keycode_to_code(0x7A), Some("F1"));
    }
}
