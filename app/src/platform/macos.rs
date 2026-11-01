//! macOS: thin FFI over CoreGraphics / AppKit to synthesize input, watch
//! generic mice and keyboards, find the frontmost app and check permissions,
//! plus login item, installed apps and Logi Options+ coexistence.

#![allow(non_upper_case_globals, clippy::missing_safety_doc)]

use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

use std::path::{Path, PathBuf};
use std::process::Command;

use logi_core::keys::{code_to_mac_keycode, mac_keycode_to_code};
use logi_core::{MediaKey, Modifier, SystemAction};

use super::{spawn, AppInfo, Capabilities, OptionsPlus, Permissions, TapEvent, TapHandler};

pub const OS: &str = "macos";
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

type CFTypeRef = *const c_void;
type CGEventRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

/// Opaque `struct __CGEvent`, so objc2's selector verification sees the right type.
#[repr(C)]
pub struct OpaqueCGEvent {
    _p: [u8; 0],
}
unsafe impl RefEncode for OpaqueCGEvent {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: CFTypeRef) -> CGEventRef;
    fn CGEventCreateKeyboardEvent(source: CFTypeRef, keycode: u16, down: bool) -> CGEventRef;
    fn CGEventCreateMouseEvent(source: CFTypeRef, ty: u32, pos: CGPoint, button: u32) -> CGEventRef;
    fn CGEventGetLocation(e: CGEventRef) -> CGPoint;
    fn CGEventSetFlags(e: CGEventRef, flags: u64);
    fn CGEventGetFlags(e: CGEventRef) -> u64;
    fn CGEventSetIntegerValueField(e: CGEventRef, field: u32, v: i64);
    fn CGEventGetIntegerValueField(e: CGEventRef, field: u32) -> i64;
    fn CGEventKeyboardSetUnicodeString(e: CGEventRef, len: usize, s: *const u16);
    fn CGEventPost(tap: u32, e: CGEventRef);
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: extern "C" fn(*mut c_void, u32, CGEventRef, *mut c_void) -> CGEventRef,
        user: *mut c_void,
    ) -> CFTypeRef;
    fn CGEventTapEnable(tap: CFTypeRef, enable: bool);
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFMachPortCreateRunLoopSource(alloc: CFTypeRef, port: CFTypeRef, order: isize) -> CFTypeRef;
    fn CFRunLoopGetCurrent() -> CFTypeRef;
    fn CFRunLoopAddSource(rl: CFTypeRef, source: CFTypeRef, mode: CFTypeRef);
    fn CFRunLoopRun();
    fn CFDictionaryCreate(
        alloc: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        n: isize,
        kcb: *const c_void,
        vcb: *const c_void,
    ) -> CFTypeRef;
    static kCFRunLoopCommonModes: CFTypeRef;
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
}

const HID_TAP: u32 = 0;
const SESSION_TAP: u32 = 1;

const EV_KEY_DOWN: u32 = 10;
const EV_KEY_UP: u32 = 11;
const EV_OTHER_DOWN: u32 = 25;
const EV_OTHER_UP: u32 = 26;
const EV_OTHER_DRAGGED: u32 = 27;
const EV_TAP_DISABLED_TIMEOUT: u32 = 0xFFFF_FFFE;
const EV_TAP_DISABLED_USER: u32 = 0xFFFF_FFFF;

const FIELD_CLICK_STATE: u32 = 1;
const FIELD_BUTTON_NUMBER: u32 = 3;
const FIELD_KEYCODE: u32 = 9;
const FIELD_USER_DATA: u32 = 42;

/// Stamped on every event we post so our own tap lets it through untouched.
const OUR_EVENTS: i64 = 0x4B4D_4F44; // "KMOD"

const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CTRL: u64 = 0x0004_0000;
const FLAG_OPTION: u64 = 0x0008_0000;
const FLAG_CMD: u64 = 0x0010_0000;
const FLAG_FN: u64 = 0x0080_0000;

fn modifier_parts(m: Modifier) -> (u16, u64) {
    match m {
        Modifier::Cmd => (0x37, FLAG_CMD),
        Modifier::Shift => (0x38, FLAG_SHIFT),
        Modifier::Option => (0x3A, FLAG_OPTION),
        Modifier::Ctrl => (0x3B, FLAG_CTRL),
        Modifier::Fn => (0x3F, FLAG_FN),
    }
}

unsafe fn post(e: CGEventRef) {
    if e.is_null() {
        return;
    }
    CGEventSetIntegerValueField(e, FIELD_USER_DATA, OUR_EVENTS);
    CGEventPost(HID_TAP, e);
    CFRelease(e);
}

fn post_key(keycode: u16, down: bool, flags: u64) {
    unsafe {
        let e = CGEventCreateKeyboardEvent(std::ptr::null(), keycode, down);
        if !e.is_null() {
            CGEventSetFlags(e, flags);
        }
        post(e);
    }
}

/// Press a key with modifiers. Modifier keys are pressed for real (not just
/// flagged) so shortcuts like ⌘Tab behave as if typed.
pub fn key_down(code: &str, mods: &[Modifier]) {
    let Some(keycode) = code_to_mac_keycode(code) else { return };
    let mut flags = 0;
    for &m in mods {
        let (kc, f) = modifier_parts(m);
        flags |= f;
        post_key(kc, true, flags);
    }
    post_key(keycode, true, flags);
}

pub fn key_up(code: &str, mods: &[Modifier]) {
    let Some(keycode) = code_to_mac_keycode(code) else { return };
    let mut flags: u64 = mods.iter().map(|&m| modifier_parts(m).1).fold(0, |a, b| a | b);
    post_key(keycode, false, flags);
    for &m in mods.iter().rev() {
        let (kc, f) = modifier_parts(m);
        flags &= !f;
        post_key(kc, false, flags);
    }
}

fn cursor() -> CGPoint {
    unsafe {
        let e = CGEventCreate(std::ptr::null());
        if e.is_null() {
            return CGPoint::default();
        }
        let p = CGEventGetLocation(e);
        CFRelease(e);
        p
    }
}

/// 0 = left, 1 = right, 2+ = "other" buttons (2 middle, 3 back, 4 forward).
pub fn mouse_button(button: u8, down: bool) {
    let (ty, cg_button) = match (button, down) {
        (0, true) => (1, 0),
        (0, false) => (2, 0),
        (1, true) => (3, 1),
        (1, false) => (4, 1),
        (b, true) => (EV_OTHER_DOWN, b as u32),
        (b, false) => (EV_OTHER_UP, b as u32),
    };
    unsafe {
        let e = CGEventCreateMouseEvent(std::ptr::null(), ty, cursor(), cg_button);
        if !e.is_null() {
            CGEventSetIntegerValueField(e, FIELD_BUTTON_NUMBER, button as i64);
            CGEventSetIntegerValueField(e, FIELD_CLICK_STATE, 1);
        }
        post(e);
    }
}

pub fn type_text(text: &str) {
    for chunk in text.encode_utf16().collect::<Vec<_>>().chunks(16) {
        for down in [true, false] {
            unsafe {
                let e = CGEventCreateKeyboardEvent(std::ptr::null(), 0, down);
                if !e.is_null() {
                    CGEventKeyboardSetUnicodeString(e, chunk.len(), chunk.as_ptr());
                }
                post(e);
            }
        }
    }
}

/// Media and brightness keys go through NSEvent "system defined" events,
/// the same path the Apple keyboard's top row uses.
pub fn media_key(key: MediaKey) {
    let code: isize = match key {
        MediaKey::VolumeUp => 0,
        MediaKey::VolumeDown => 1,
        MediaKey::BrightnessUp => 2,
        MediaKey::BrightnessDown => 3,
        MediaKey::Mute => 7,
        MediaKey::PlayPause => 16,
        MediaKey::Next => 17,
        MediaKey::Previous => 18,
        MediaKey::KeyboardBrightnessUp => 21,
        MediaKey::KeyboardBrightnessDown => 22,
    };
    for down in [true, false] {
        let state: isize = if down { 0xA } else { 0xB };
        objc2::rc::autoreleasepool(|_| unsafe {
            let ev: *mut AnyObject = msg_send![
                class!(NSEvent),
                otherEventWithType: 14usize,
                location: CGPoint::default(),
                modifierFlags: (state << 8) as usize,
                timestamp: 0.0f64,
                windowNumber: 0isize,
                context: std::ptr::null_mut::<AnyObject>(),
                subtype: 8i16,
                data1: (code << 16) | (state << 8),
                data2: -1isize
            ];
            if ev.is_null() {
                return;
            }
            let cg: *mut OpaqueCGEvent = msg_send![ev, CGEvent];
            if !cg.is_null() {
                CGEventSetIntegerValueField(cg.cast(), FIELD_USER_DATA, OUR_EVENTS);
                CGEventPost(HID_TAP, cg.cast());
            }
        });
    }
}

/// Bundle id of the frontmost application, for per-app bindings.
pub fn frontmost_app() -> Option<String> {
    objc2::rc::autoreleasepool(|_| unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let bid: *mut AnyObject = msg_send![app, bundleIdentifier];
        if bid.is_null() {
            return None;
        }
        let s: *const c_char = msg_send![bid, UTF8String];
        (!s.is_null()).then(|| CStr::from_ptr(s).to_string_lossy().into_owned())
    })
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

pub fn permissions() -> Permissions {
    Permissions { accessibility: accessibility_trusted(false), input_monitoring: input_monitoring_granted() }
}

/// Ask for a permission; opens System Settings when a prompt isn't possible.
pub fn request_permission(kind: &str) -> bool {
    let ok = match kind {
        "accessibility" => accessibility_trusted(true),
        "input" => request_input_monitoring(),
        _ => return false,
    };
    if !ok {
        open_settings(kind);
    }
    ok
}

/// Whether the any-brand hook can be installed right now.
pub fn event_tap_ready() -> bool {
    accessibility_trusted(false)
}

/// Accessibility: needed to post synthetic input and to filter events.
fn accessibility_trusted(prompt: bool) -> bool {
    unsafe {
        if !prompt {
            return AXIsProcessTrustedWithOptions(std::ptr::null());
        }
        let keys = [kAXTrustedCheckOptionPrompt];
        let vals = [kCFBooleanTrue];
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const c_void,
        );
        let ok = AXIsProcessTrustedWithOptions(dict);
        if !dict.is_null() {
            CFRelease(dict);
        }
        ok
    }
}

/// Input Monitoring: needed to read input from HID devices.
fn input_monitoring_granted() -> bool {
    // kIOHIDAccessTypeGranted = 0, kIOHIDRequestTypeListenEvent = 1
    unsafe { IOHIDCheckAccess(1) == 0 || CGPreflightListenEventAccess() }
}

fn request_input_monitoring() -> bool {
    unsafe { CGRequestListenEventAccess() }
}

// ---------------------------------------------------------------------------
// Generic devices: an event tap that sees every mouse button and key.
// ---------------------------------------------------------------------------

static TAP_HANDLER: OnceLock<TapHandler> = OnceLock::new();
static TAP_PORT: OnceLock<usize> = OnceLock::new();

extern "C" fn tap_callback(_proxy: *mut c_void, ty: u32, event: CGEventRef, _user: *mut c_void) -> CGEventRef {
    unsafe {
        if ty == EV_TAP_DISABLED_TIMEOUT || ty == EV_TAP_DISABLED_USER {
            if let Some(&port) = TAP_PORT.get() {
                CGEventTapEnable(port as CFTypeRef, true);
            }
            return event;
        }
        if CGEventGetIntegerValueField(event, FIELD_USER_DATA) == OUR_EVENTS {
            return event;
        }
        let Some(handler) = TAP_HANDLER.get() else { return event };
        let ev = match ty {
            EV_OTHER_DOWN | EV_OTHER_UP => {
                let button = CGEventGetIntegerValueField(event, FIELD_BUTTON_NUMBER) as u8;
                TapEvent::MouseButton { button, down: ty == EV_OTHER_DOWN }
            }
            EV_OTHER_DRAGGED => return event,
            EV_KEY_DOWN | EV_KEY_UP => {
                let keycode = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
                let Some(code) = mac_keycode_to_code(keycode) else { return event };
                let flags = CGEventGetFlags(event);
                TapEvent::Key { code, down: ty == EV_KEY_DOWN, modified: flags & (FLAG_CMD | FLAG_CTRL | FLAG_OPTION | FLAG_SHIFT) != 0 }
            }
            _ => return event,
        };
        if handler(ev) {
            std::ptr::null_mut()
        } else {
            event
        }
    }
}

/// Install the tap on its own run-loop thread. Fails without Accessibility.
pub fn start_event_tap(handler: TapHandler) -> bool {
    if TAP_PORT.get().is_some() {
        return true;
    }
    let _ = TAP_HANDLER.set(handler);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("event-tap".into())
        .spawn(move || unsafe {
            let mask: u64 = (1 << EV_OTHER_DOWN) | (1 << EV_OTHER_UP) | (1 << EV_KEY_DOWN) | (1 << EV_KEY_UP);
            let port = CGEventTapCreate(SESSION_TAP, 0, 0, mask, tap_callback, std::ptr::null_mut());
            if port.is_null() {
                let _ = tx.send(false);
                return;
            }
            let _ = TAP_PORT.set(port as usize);
            let src = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(port, true);
            let _ = tx.send(true);
            CFRunLoopRun();
        })
        .ok();
    rx.recv().unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Actions that are really OS features
// ---------------------------------------------------------------------------

pub fn capabilities() -> Capabilities {
    Capabilities { os: OS, per_app: true, any_device: true, options_plus: true, unsupported_system: vec![], unsupported_media: vec![] }
}

/// Perform a system action. Returns false when this OS has no equivalent.
pub fn system_action(a: SystemAction) -> bool {
    use Modifier::*;
    let keys = |code: &str, mods: &[Modifier]| super::key_tap(code, mods);
    match a {
        SystemAction::MissionControl => spawn("open", &["-a", "Mission Control"]),
        SystemAction::ShowDesktop => mission_control("1"),
        SystemAction::AppExpose => mission_control("2"),
        SystemAction::Launchpad => spawn("/bin/sh", &["-c", "open -a Launchpad 2>/dev/null || open -a Apps"]),
        SystemAction::Spotlight => keys("Space", &[Cmd]),
        SystemAction::Screenshot => keys("Digit3", &[Cmd, Shift]),
        SystemAction::ScreenshotRegion => keys("Digit4", &[Cmd, Shift]),
        SystemAction::LockScreen => keys("KeyQ", &[Ctrl, Cmd]),
        SystemAction::EmojiPicker => keys("Space", &[Ctrl, Cmd]),
        SystemAction::NavigateBack => keys("BracketLeft", &[Cmd]),
        SystemAction::NavigateForward => keys("BracketRight", &[Cmd]),
        SystemAction::NextSpace => keys("ArrowRight", &[Ctrl]),
        SystemAction::PreviousSpace => keys("ArrowLeft", &[Ctrl]),
        SystemAction::SwitchApp => keys("Tab", &[Cmd]),
        SystemAction::ToggleMicrophone => toggle_microphone(),
    }
    true
}

fn mission_control(arg: &str) {
    spawn("/System/Applications/Mission Control.app/Contents/MacOS/Mission Control", &[arg]);
}

/// Mute by setting input volume to zero; unmute restores the previous level.
fn toggle_microphone() {
    static SAVED: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);
    std::thread::spawn(|| {
        let out = Command::new("osascript").args(["-e", "input volume of (get volume settings)"]).output();
        let current: u32 = out.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok()).unwrap_or(0);
        let mut saved = SAVED.lock().unwrap();
        let target = if current > 0 {
            *saved = Some(current);
            0
        } else {
            saved.take().unwrap_or(75)
        };
        let _ = Command::new("osascript").args(["-e", &format!("set volume input volume {target}")]).status();
    });
}

/// Open an application bundle, file or URL.
pub fn open(target: &str) {
    spawn("open", &[target]);
}

pub fn run_shell(command: &str) {
    spawn("/bin/sh", &["-c", command]);
}

/// Show a folder in Finder.
pub fn reveal(path: &Path) {
    let _ = Command::new("open").arg(path).status();
}

/// Show a file selected in Finder.
pub fn reveal_file(path: &Path) {
    let _ = Command::new("open").arg("-R").arg(path).status();
}

// ---------------------------------------------------------------------------
// Integration: Logi Options+, login item, installed apps, settings links
// ---------------------------------------------------------------------------

const OPTIONS_PLUS_AGENT_PLIST: &str = "/Library/LaunchAgents/com.logi.optionsplus.plist";
const OPTIONS_PLUS_LABEL: &str = "com.logi.optionsplus";
const LOGIN_LABEL: &str = "dev.keymodus.app";

pub fn options_plus() -> OptionsPlus {
    OptionsPlus {
        installed: Path::new(OPTIONS_PLUS_AGENT_PLIST).exists() || Path::new("/Applications/logioptionsplus.app").exists(),
        running: is_running("logioptionsplus_agent"),
    }
}

fn is_running(name: &str) -> bool {
    Command::new("/usr/bin/pgrep").args(["-x", name]).output().map(|o| o.status.success()).unwrap_or(false)
}

fn uid() -> String {
    Command::new("/usr/bin/id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// Stop the Options+ agent for this login session. Both apps diverting the
/// same buttons would fight; this is reversible with [`resume_options_plus`]
/// and Options+ comes back on its own at next login.
pub fn pause_options_plus() -> Result<(), String> {
    let domain = format!("gui/{}", uid());
    let _ = Command::new("/bin/launchctl").args(["bootout", &format!("{domain}/{OPTIONS_PLUS_LABEL}")]).status();
    let _ = Command::new("/usr/bin/pkill").args(["-x", "logioptionsplus"]).status();
    let _ = Command::new("/usr/bin/pkill").args(["-x", "logioptionsplus_agent"]).status();
    std::thread::sleep(std::time::Duration::from_millis(600));
    if is_running("logioptionsplus_agent") {
        Err("Options+ is still running. Quit it from its menu bar icon and try again.".into())
    } else {
        Ok(())
    }
}

pub fn resume_options_plus() -> Result<(), String> {
    let domain = format!("gui/{}", uid());
    let st = Command::new("/bin/launchctl").args(["bootstrap", &domain, OPTIONS_PLUS_AGENT_PLIST]).status().map_err(|e| e.to_string())?;
    if st.success() || is_running("logioptionsplus_agent") {
        Ok(())
    } else {
        Err("Could not start Options+ again. It will start at next login.".into())
    }
}

fn login_plist() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(format!("Library/LaunchAgents/{LOGIN_LABEL}.plist"))
}

/// Install or remove a per-user LaunchAgent that starts us in the background.
pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    let path = login_plist();
    if !on {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().replace('&', "&amp;").replace('<', "&lt;");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LOGIN_LABEL}</string>
  <key>ProgramArguments</key><array><string>{exe}</string><string>--background</string></array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, plist).map_err(|e| e.to_string())
}

/// Point an existing login item at this copy of the app, so it survives the
/// app being moved or reinstalled to a different folder.
pub fn refresh_login_item() {
    let in_bundle = std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"));
    // Replace the login item left by the app's previous name (Logidriver).
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let legacy = home.join("Library/LaunchAgents/dev.logidriver.app.plist");
    if in_bundle && legacy.exists() && std::fs::remove_file(&legacy).is_ok() {
        let _ = set_launch_at_login(true);
    }
    if in_bundle && launch_at_login() {
        let _ = set_launch_at_login(true);
    }
}

pub fn launch_at_login() -> bool {
    login_plist().exists()
}

pub fn installed_apps() -> Vec<AppInfo> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let roots = [PathBuf::from("/Applications"), PathBuf::from("/System/Applications"), home.join("Applications")];
    let mut out = vec![];
    for root in roots {
        collect_apps(&root, 2, &mut out);
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.bundle_id == b.bundle_id);
    out
}

fn collect_apps(dir: &Path, depth: u8, out: &mut Vec<AppInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "app") {
            if let Some(bundle_id) = logi_core::import::bundle_id_of(&p) {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(AppInfo { name, bundle_id, path: p.to_string_lossy().into_owned() });
            }
        } else if depth > 1 && p.is_dir() {
            collect_apps(&p, depth - 1, out);
        }
    }
}

pub fn open_settings(pane: &str) {
    let url = match pane {
        "accessibility" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        "input" => "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
        "bluetooth" => "x-apple.systempreferences:com.apple.BluetoothSettings",
        _ => return,
    };
    let _ = Command::new("open").arg(url).status();
}
