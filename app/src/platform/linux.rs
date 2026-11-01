//! Linux: a uinput virtual device for synthesis, X11 for the frontmost app
//! (Wayland compositors don't expose it, so per-app profiles fall back to the
//! global bindings there), XDG autostart for launch at login.
//!
//! Access to `/dev/uinput` and the Logitech hidraw nodes comes from the udev
//! rule in `packaging/linux/70-keymodus.rules`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{AttributeSet, EventType, InputEvent, Key, RelativeAxisType};
use logi_core::keys::{all_codes, code_to_evdev};
use logi_core::{MediaKey, Modifier, SystemAction};

use super::{spawn, AppInfo, Capabilities, OptionsPlus, Permissions, TapHandler};

pub const OS: &str = "linux";

const MEDIA_KEYS: [Key; 10] = [
    Key::KEY_PLAYPAUSE,
    Key::KEY_NEXTSONG,
    Key::KEY_PREVIOUSSONG,
    Key::KEY_VOLUMEUP,
    Key::KEY_VOLUMEDOWN,
    Key::KEY_MUTE,
    Key::KEY_BRIGHTNESSUP,
    Key::KEY_BRIGHTNESSDOWN,
    Key::KEY_KBDILLUMUP,
    Key::KEY_KBDILLUMDOWN,
];
const MOD_KEYS: [Key; 4] = [Key::KEY_LEFTMETA, Key::KEY_LEFTALT, Key::KEY_LEFTCTRL, Key::KEY_LEFTSHIFT];
const BUTTONS: [Key; 5] = [Key::BTN_LEFT, Key::BTN_RIGHT, Key::BTN_MIDDLE, Key::BTN_SIDE, Key::BTN_EXTRA];

// ---------------------------------------------------------------------------
// uinput virtual device
// ---------------------------------------------------------------------------

fn build_device() -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<Key>::new();
    for code in all_codes().filter_map(code_to_evdev) {
        keys.insert(Key::new(code));
    }
    for k in MEDIA_KEYS.iter().chain(&MOD_KEYS).chain(&BUTTONS) {
        keys.insert(*k);
    }
    // Relative axes make desktops treat the device as a mouse, so buttons work.
    let mut rel = AttributeSet::<RelativeAxisType>::new();
    rel.insert(RelativeAxisType::REL_X);
    rel.insert(RelativeAxisType::REL_Y);
    VirtualDeviceBuilder::new()?.name("Keymodus virtual input").with_keys(&keys)?.with_relative_axes(&rel)?.build()
}

/// Send key events (code, value) followed by a SYN report. No-op without uinput access.
fn emit(events: &[(u16, i32)]) {
    static DEVICE: OnceLock<Mutex<Option<VirtualDevice>>> = OnceLock::new();
    let dev = DEVICE.get_or_init(|| {
        let d = build_device().ok();
        if d.is_some() {
            // Give the desktop a moment to pick up the new device.
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        Mutex::new(d)
    });
    let Ok(mut guard) = dev.lock() else { return };
    if let Some(d) = guard.as_mut() {
        let evs: Vec<InputEvent> = events.iter().map(|&(c, v)| InputEvent::new(EventType::KEY, c, v)).collect();
        let _ = d.emit(&evs);
    }
}

fn modifier_key(m: Modifier) -> Option<Key> {
    match m {
        Modifier::Cmd => Some(Key::KEY_LEFTMETA),
        Modifier::Option => Some(Key::KEY_LEFTALT),
        Modifier::Ctrl => Some(Key::KEY_LEFTCTRL),
        Modifier::Shift => Some(Key::KEY_LEFTSHIFT),
        Modifier::Fn => None,
    }
}

fn press_codes(key: u16, mods: &[Modifier], down: bool) {
    let mods: Vec<u16> = mods.iter().filter_map(|&m| modifier_key(m)).map(|k| k.code()).collect();
    let mut v = vec![];
    if down {
        v.extend(mods.iter().map(|&m| (m, 1)));
        v.push((key, 1));
    } else {
        v.push((key, 0));
        v.extend(mods.iter().rev().map(|&m| (m, 0)));
    }
    emit(&v);
}

pub fn key_down(code: &str, mods: &[Modifier]) {
    if let Some(k) = code_to_evdev(code) {
        press_codes(k, mods, true);
    }
}

pub fn key_up(code: &str, mods: &[Modifier]) {
    if let Some(k) = code_to_evdev(code) {
        press_codes(k, mods, false);
    }
}

fn tap_key(key: Key, mods: &[Modifier]) {
    press_codes(key.code(), mods, true);
    press_codes(key.code(), mods, false);
}

/// 0 = left, 1 = right, 2 = middle, 3 = back, 4 = forward.
pub fn mouse_button(button: u8, down: bool) {
    if let Some(b) = BUTTONS.get(button as usize) {
        emit(&[(b.code(), down as i32)]);
    }
}

pub fn type_text(text: &str) {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        spawn("wtype", &["--", text]);
    } else {
        spawn("xdotool", &["type", "--clearmodifiers", "--", text]);
    }
}

pub fn media_key(key: MediaKey) {
    let k = match key {
        MediaKey::PlayPause => Key::KEY_PLAYPAUSE,
        MediaKey::Next => Key::KEY_NEXTSONG,
        MediaKey::Previous => Key::KEY_PREVIOUSSONG,
        MediaKey::VolumeUp => Key::KEY_VOLUMEUP,
        MediaKey::VolumeDown => Key::KEY_VOLUMEDOWN,
        MediaKey::Mute => Key::KEY_MUTE,
        MediaKey::BrightnessUp => Key::KEY_BRIGHTNESSUP,
        MediaKey::BrightnessDown => Key::KEY_BRIGHTNESSDOWN,
        MediaKey::KeyboardBrightnessUp => Key::KEY_KBDILLUMUP,
        MediaKey::KeyboardBrightnessDown => Key::KEY_KBDILLUMDOWN,
    };
    tap_key(k, &[]);
}

// ---------------------------------------------------------------------------
// Context and permissions
// ---------------------------------------------------------------------------

fn is_x11() -> bool {
    std::env::var_os("DISPLAY").is_some() && std::env::var_os("WAYLAND_DISPLAY").is_none()
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(program).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Lower-case WM_CLASS class of the active X11 window. None under Wayland.
pub fn frontmost_app() -> Option<String> {
    if !is_x11() {
        return None;
    }
    let root = output("xprop", &["-root", "_NET_ACTIVE_WINDOW"])?;
    let id = root.split_whitespace().last()?.to_string();
    if !id.starts_with("0x") || id == "0x0" {
        return None;
    }
    let class = output("xprop", &["-id", &id, "WM_CLASS"])?;
    // WM_CLASS(STRING) = "instance", "Class"
    let parts: Vec<&str> = class.split('"').filter(|s| !s.trim().is_empty() && !s.contains('=') && s.trim() != ",").collect();
    parts.get(1).or(parts.first()).map(|s| s.to_lowercase())
}

fn uinput_writable() -> bool {
    fs::OpenOptions::new().write(true).open("/dev/uinput").is_ok()
}

pub fn permissions() -> Permissions {
    Permissions { accessibility: uinput_writable(), input_monitoring: true }
}

/// Nothing to prompt for on Linux; access comes from the udev rule.
pub fn request_permission(kind: &str) -> bool {
    match kind {
        "accessibility" => uinput_writable(),
        _ => true,
    }
}

pub fn event_tap_ready() -> bool {
    false
}

/// The any-brand hook is not available on Linux yet.
pub fn start_event_tap(_handler: TapHandler) -> bool {
    false
}

pub fn capabilities() -> Capabilities {
    Capabilities { os: OS, per_app: is_x11(), any_device: false, options_plus: false, unsupported_system: vec![], unsupported_media: vec![] }
}

// ---------------------------------------------------------------------------
// System actions (GNOME default shortcuts)
// ---------------------------------------------------------------------------

pub fn system_action(a: SystemAction) -> bool {
    use Modifier::*;
    let keys = |code: &str, mods: &[Modifier]| super::key_tap(code, mods);
    match a {
        SystemAction::MissionControl | SystemAction::AppExpose | SystemAction::Spotlight => tap_key(Key::KEY_LEFTMETA, &[]),
        SystemAction::ShowDesktop => keys("KeyD", &[Cmd]),
        SystemAction::Launchpad => keys("KeyA", &[Cmd]),
        SystemAction::Screenshot => keys("PrintScreen", &[]),
        SystemAction::ScreenshotRegion => keys("PrintScreen", &[Shift]),
        SystemAction::LockScreen => keys("KeyL", &[Cmd]),
        SystemAction::EmojiPicker => keys("Period", &[Cmd]),
        SystemAction::NavigateBack => keys("ArrowLeft", &[Option]),
        SystemAction::NavigateForward => keys("ArrowRight", &[Option]),
        SystemAction::NextSpace => keys("PageDown", &[Cmd]),
        SystemAction::PreviousSpace => keys("PageUp", &[Cmd]),
        SystemAction::SwitchApp => keys("Tab", &[Option]),
        SystemAction::ToggleMicrophone => spawn("pactl", &["set-source-mute", "@DEFAULT_SOURCE@", "toggle"]),
    }
    true
}

// ---------------------------------------------------------------------------
// Opening things
// ---------------------------------------------------------------------------

pub fn open(target: &str) {
    spawn("xdg-open", &[target]);
}

pub fn run_shell(command: &str) {
    spawn("/bin/sh", &["-c", command]);
}

pub fn reveal(path: &Path) {
    if let Some(p) = path.to_str() {
        spawn("xdg-open", &[p]);
    }
}

pub fn reveal_file(path: &Path) {
    if let Some(parent) = path.parent() {
        reveal(parent);
    }
}

pub fn open_settings(pane: &str) {
    if pane == "bluetooth" {
        spawn("gnome-control-center", &["bluetooth"]);
    }
}

// ---------------------------------------------------------------------------
// Logi Options+ (not available on Linux), login item, apps
// ---------------------------------------------------------------------------

pub fn options_plus() -> OptionsPlus {
    OptionsPlus { installed: false, running: false }
}

pub fn pause_options_plus() -> Result<(), String> {
    Err("Logi Options+ is not available on Linux.".into())
}

pub fn resume_options_plus() -> Result<(), String> {
    Err("Logi Options+ is not available on Linux.".into())
}

fn autostart_file() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("autostart/keymodus.desktop")
}

fn launcher_path() -> Option<PathBuf> {
    // An AppImage runs from a temporary mount; the stable path is in $APPIMAGE.
    std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok())
}

pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    let path = autostart_file();
    if !on {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let exe = launcher_path().ok_or("Could not find the Keymodus executable")?;
    let exe = exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Keymodus\nComment=Mouse and keyboard configuration\nExec=\"{exe}\" --background\nX-GNOME-Autostart-enabled=true\nTerminal=false\n"
    );
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, entry).map_err(|e| e.to_string())
}

pub fn launch_at_login() -> bool {
    autostart_file().exists()
}

/// Point an existing login item at this copy of the app.
pub fn refresh_login_item() {
    if launch_at_login() {
        let _ = set_launch_at_login(true);
    }
}

pub fn installed_apps() -> Vec<AppInfo> {
    let mut roots = vec![PathBuf::from("/usr/share/applications"), PathBuf::from("/var/lib/flatpak/exports/share/applications")];
    if let Some(data) = dirs::data_dir() {
        roots.push(data.join("applications"));
    }
    let mut out = vec![];
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "desktop") {
                if let Some(app) = parse_desktop(&p) {
                    out.push(app);
                }
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.bundle_id == b.bundle_id);
    out
}

fn parse_desktop(path: &Path) -> Option<AppInfo> {
    let text = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let (mut name, mut wm_class, mut hidden) = (None, None, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            name.get_or_insert_with(|| v.to_string());
        } else if let Some(v) = line.strip_prefix("StartupWMClass=") {
            wm_class = Some(v.to_lowercase());
        } else if line == "NoDisplay=true" || line == "Hidden=true" {
            hidden = true;
        }
    }
    if hidden {
        return None;
    }
    let stem = path.file_stem()?.to_string_lossy().to_lowercase();
    Some(AppInfo { name: name?, bundle_id: wm_class.unwrap_or(stem), path: path.to_string_lossy().into_owned() })
}
