//! Windows: SendInput for synthesis, low-level hooks for the "any brand"
//! devices, the foreground process for per-app profiles, and the HKCU Run key
//! for launch at login.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use logi_core::keys::{code_to_windows_vk, windows_vk_to_code};
use logi_core::{MediaKey, Modifier, SystemAction};
use windows_sys::Win32::Foundation::{CloseHandle, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, VK_CONTROL, VK_LWIN, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MENU,
    VK_RWIN, VK_SHIFT, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, SetWindowsHookExW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED,
    MSG, MSLLHOOKSTRUCT, SW_SHOWNORMAL, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use super::{spawn, AppInfo, Capabilities, OptionsPlus, Permissions, TapEvent, TapHandler};

pub const OS: &str = "windows";

/// Stamped on every event we inject so our own hooks let it through.
const OUR_EVENTS: usize = 0x4B4D_4F44; // "KMOD"
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const XBUTTON1: u32 = 1;
const XBUTTON2: u32 = 2;

// ---------------------------------------------------------------------------
// Input synthesis
// ---------------------------------------------------------------------------

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: OUR_EVENTS } } }
}

fn is_extended(vk: u16) -> bool {
    // PageUp, PageDown, End, Home, arrows, Insert, Delete, Windows keys.
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E) || vk == VK_LWIN || vk == VK_RWIN
}

fn vk_event(vk: u16, down: bool) -> INPUT {
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    key_input(vk, 0, flags)
}

fn modifier_vk(m: Modifier) -> Option<u16> {
    match m {
        Modifier::Cmd => Some(VK_LWIN),
        Modifier::Option => Some(VK_MENU),
        Modifier::Ctrl => Some(VK_CONTROL),
        Modifier::Shift => Some(VK_SHIFT),
        Modifier::Fn => None,
    }
}

fn press_vks(vk: u16, mods: &[Modifier], down: bool) {
    let mods: Vec<u16> = mods.iter().filter_map(|&m| modifier_vk(m)).collect();
    let mut v = vec![];
    if down {
        v.extend(mods.iter().map(|&m| vk_event(m, true)));
        v.push(vk_event(vk, true));
    } else {
        v.push(vk_event(vk, false));
        v.extend(mods.iter().rev().map(|&m| vk_event(m, false)));
    }
    send(&v);
}

pub fn key_down(code: &str, mods: &[Modifier]) {
    if let Some(vk) = code_to_windows_vk(code) {
        press_vks(vk, mods, true);
    }
}

pub fn key_up(code: &str, mods: &[Modifier]) {
    if let Some(vk) = code_to_windows_vk(code) {
        press_vks(vk, mods, false);
    }
}

fn vk_tap(vk: u16, mods: &[Modifier]) {
    press_vks(vk, mods, true);
    press_vks(vk, mods, false);
}

/// 0 = left, 1 = right, 2 = middle, 3 = back, 4 = forward.
pub fn mouse_button(button: u8, down: bool) {
    let (flags, data) = match (button, down) {
        (0, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (0, false) => (MOUSEEVENTF_LEFTUP, 0),
        (1, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (1, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (2, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (2, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (3, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
        (3, false) => (MOUSEEVENTF_XUP, XBUTTON1),
        (4, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
        (4, false) => (MOUSEEVENTF_XUP, XBUTTON2),
        _ => return,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data as _, dwFlags: flags, time: 0, dwExtraInfo: OUR_EVENTS } },
    };
    send(&[input]);
}

pub fn type_text(text: &str) {
    let mut v = vec![];
    for unit in text.encode_utf16() {
        v.push(key_input(0, unit, KEYEVENTF_UNICODE));
        v.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    for chunk in v.chunks(64) {
        send(chunk);
    }
}

pub fn media_key(key: MediaKey) {
    let vk = match key {
        MediaKey::PlayPause => VK_MEDIA_PLAY_PAUSE,
        MediaKey::Next => VK_MEDIA_NEXT_TRACK,
        MediaKey::Previous => VK_MEDIA_PREV_TRACK,
        MediaKey::VolumeUp => VK_VOLUME_UP,
        MediaKey::VolumeDown => VK_VOLUME_DOWN,
        MediaKey::Mute => VK_VOLUME_MUTE,
        // No standard virtual key; see capabilities().
        MediaKey::BrightnessUp | MediaKey::BrightnessDown | MediaKey::KeyboardBrightnessUp | MediaKey::KeyboardBrightnessDown => return,
    };
    send(&[key_input(vk, 0, KEYEVENTF_EXTENDEDKEY), key_input(vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP)]);
}

// ---------------------------------------------------------------------------
// Context and permissions
// ---------------------------------------------------------------------------

/// Lower-case executable name of the foreground window's process, e.g. "chrome.exe".
pub fn frontmost_app() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&path).file_name().map(|n| n.to_string_lossy().to_lowercase())
    }
}

pub fn permissions() -> Permissions {
    Permissions { accessibility: true, input_monitoring: true }
}

pub fn request_permission(_kind: &str) -> bool {
    true
}

pub fn event_tap_ready() -> bool {
    true
}

pub fn capabilities() -> Capabilities {
    Capabilities {
        os: OS,
        per_app: true,
        any_device: true,
        options_plus: false,
        unsupported_system: vec![],
        unsupported_media: vec!["brightness_up", "brightness_down", "keyboard_brightness_up", "keyboard_brightness_down"],
    }
}

// ---------------------------------------------------------------------------
// Any-brand hook: low-level mouse and keyboard hooks
// ---------------------------------------------------------------------------

static TAP_HANDLER: OnceLock<TapHandler> = OnceLock::new();
static TAP_STARTED: OnceLock<bool> = OnceLock::new();

fn modifier_held() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|&vk| unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 })
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        let ours = info.flags & LLMHF_INJECTED != 0 && info.dwExtraInfo == OUR_EVENTS;
        if !ours {
            let msg = wparam as u32;
            let x = (info.mouseData >> 16) & 0xFFFF;
            let ev = match msg {
                WM_MBUTTONDOWN => Some((2, true)),
                WM_MBUTTONUP => Some((2, false)),
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let b = if x == XBUTTON1 { 3 } else { 4 };
                    Some((b, msg == WM_XBUTTONDOWN))
                }
                _ => None,
            };
            if let (Some((button, down)), Some(h)) = (ev, TAP_HANDLER.get()) {
                if h(TapEvent::MouseButton { button, down }) {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        let ours = info.flags & LLKHF_INJECTED != 0 && info.dwExtraInfo == OUR_EVENTS;
        if !ours {
            let msg = wparam as u32;
            let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
            let up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
            if down || up {
                if let (Some(code), Some(h)) = (windows_vk_to_code(info.vkCode as u16), TAP_HANDLER.get()) {
                    if h(TapEvent::Key { code, down, modified: modifier_held() }) {
                        return 1;
                    }
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Install both hooks on a dedicated message-loop thread.
pub fn start_event_tap(handler: TapHandler) -> bool {
    if let Some(&ok) = TAP_STARTED.get() {
        return ok;
    }
    let _ = TAP_HANDLER.set(handler);
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("input-hook".into()).spawn(move || unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
        let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
        let ok = !m.is_null() && !k.is_null();
        let _ = tx.send(ok);
        if !ok {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
    });
    let ok = spawned.is_ok() && rx.recv().unwrap_or(false);
    let _ = TAP_STARTED.set(ok);
    ok
}

// ---------------------------------------------------------------------------
// System actions
// ---------------------------------------------------------------------------

pub fn system_action(a: SystemAction) -> bool {
    use Modifier::*;
    let keys = |code: &str, mods: &[Modifier]| super::key_tap(code, mods);
    match a {
        SystemAction::MissionControl | SystemAction::AppExpose => keys("Tab", &[Cmd]),
        SystemAction::ShowDesktop => keys("KeyD", &[Cmd]),
        SystemAction::Launchpad => vk_tap(VK_LWIN, &[]),
        SystemAction::Spotlight => keys("KeyS", &[Cmd]),
        SystemAction::Screenshot => keys("PrintScreen", &[]),
        SystemAction::ScreenshotRegion => keys("KeyS", &[Cmd, Shift]),
        SystemAction::LockScreen => keys("KeyL", &[Cmd]),
        SystemAction::EmojiPicker => keys("Period", &[Cmd]),
        SystemAction::NavigateBack => keys("ArrowLeft", &[Option]),
        SystemAction::NavigateForward => keys("ArrowRight", &[Option]),
        SystemAction::NextSpace => keys("ArrowRight", &[Ctrl, Cmd]),
        SystemAction::PreviousSpace => keys("ArrowLeft", &[Ctrl, Cmd]),
        SystemAction::SwitchApp => keys("Tab", &[Option]),
        SystemAction::ToggleMicrophone => keys("KeyK", &[Cmd, Option]),
    }
    true
}

// ---------------------------------------------------------------------------
// Opening things
// ---------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Open an application, file or URL with its default handler. ShellExecuteW
/// takes the target as one argument, so no shell parsing is involved.
pub fn open(target: &str) {
    let op = wide("open");
    let file = wide(target);
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

fn hidden(program: &str) -> Command {
    let mut c = Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

pub fn run_shell(command: &str) {
    let child = hidden("cmd").arg("/C").raw_arg(command).spawn();
    if let Ok(mut c) = child {
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}

pub fn reveal(path: &Path) {
    spawn("explorer", &[&path.to_string_lossy()]);
}

pub fn reveal_file(path: &Path) {
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(path);
    let _ = Command::new("explorer").arg(arg).spawn();
}

pub fn open_settings(pane: &str) {
    if pane == "bluetooth" {
        open("ms-settings:bluetooth");
    }
}

// ---------------------------------------------------------------------------
// Logi Options+, login item, apps
// ---------------------------------------------------------------------------

fn running_images() -> Vec<String> {
    let Ok(out) = hidden("tasklist").args(["/fo", "csv", "/nh"]).output() else { return vec![] };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('"').and_then(|l| l.split('"').next()).map(str::to_string))
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn options_plus() -> OptionsPlus {
    let pf = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Program Files".into());
    OptionsPlus {
        installed: pf.join("LogiOptionsPlus").exists(),
        running: running_images().iter().any(|n| n.eq_ignore_ascii_case("logioptionsplus_agent.exe")),
    }
}

const OPTIONS_PLUS_UNSUPPORTED: &str = "Pausing Options+ is only supported on macOS. Quit it from the system tray.";

pub fn pause_options_plus() -> Result<(), String> {
    Err(OPTIONS_PLUS_UNSUPPORTED.into())
}

pub fn resume_options_plus() -> Result<(), String> {
    Err(OPTIONS_PLUS_UNSUPPORTED.into())
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Keymodus";

pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    let status = if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let data = format!("\"{}\" --background", exe.display());
        hidden("reg").args(["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &data, "/f"]).status()
    } else {
        if !launch_at_login() {
            return Ok(());
        }
        hidden("reg").args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"]).status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err("Could not update the login item".into()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn launch_at_login() -> bool {
    hidden("reg").args(["query", RUN_KEY, "/v", RUN_VALUE]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Point an existing login item at this copy of the app.
pub fn refresh_login_item() {
    if launch_at_login() {
        let _ = set_launch_at_login(true);
    }
}

const SYSTEM_PROCESSES: &[&str] = &[
    "system",
    "system idle process",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "fontdrvhost.exe",
    "dwm.exe",
    "conhost.exe",
    "sihost.exe",
    "taskhostw.exe",
    "ctfmon.exe",
    "runtimebroker.exe",
    "dllhost.exe",
    "searchhost.exe",
    "searchindexer.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "wmiprvse.exe",
    "audiodg.exe",
    "spoolsv.exe",
    "securityhealthservice.exe",
    "msmpeng.exe",
    "nissrv.exe",
    "lsaiso.exe",
    "memory compression",
    "tasklist.exe",
    "smartscreen.exe",
    "applicationframehost.exe",
    "systemsettingsbroker.exe",
];

/// Apps for per-app profiles: whatever is running now, by executable name.
pub fn installed_apps() -> Vec<AppInfo> {
    let mut out: Vec<AppInfo> = running_images()
        .into_iter()
        .map(|n| n.to_lowercase())
        .filter(|n| !SYSTEM_PROCESSES.contains(&n.as_str()))
        .map(|id| {
            let stem = id.strip_suffix(".exe").unwrap_or(&id);
            let mut name: Vec<char> = stem.chars().collect();
            if let Some(c) = name.first_mut() {
                *c = c.to_ascii_uppercase();
            }
            AppInfo { name: name.into_iter().collect(), bundle_id: id.clone(), path: String::new() }
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.bundle_id == b.bundle_id);
    out
}
