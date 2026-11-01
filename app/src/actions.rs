//! Turn an [`Action`] into input or a side effect, through the platform layer.

use std::time::Duration;

use logi_core::config::{MAX_STEPS, MAX_STEP_DELAY_MS};
use logi_core::{Action, Step};

use crate::platform;

/// Whether an action is performed as press-and-hold (down on press, up on
/// release) rather than fired once on press.
pub fn is_holdable(a: &Action) -> bool {
    matches!(a, Action::Keystroke(_) | Action::MouseButton { .. })
}

pub fn press(a: &Action) {
    match a {
        Action::Keystroke(k) => platform::key_down(&k.key, &k.mods),
        Action::MouseButton { button } => platform::mouse_button(*button, true),
        other => fire(other),
    }
}

pub fn release(a: &Action) {
    match a {
        Action::Keystroke(k) => platform::key_up(&k.key, &k.mods),
        Action::MouseButton { button } => platform::mouse_button(*button, false),
        _ => {}
    }
}

/// Press and release in one go.
pub fn tap(a: &Action) {
    press(a);
    release(a);
}

/// One-shot actions. `CycleDpi`, `Gesture` and `SmartAction` need device or
/// config context and are handled by the engine.
fn fire(a: &Action) {
    match a {
        Action::Default | Action::Disabled | Action::CycleDpi | Action::Gesture(_) | Action::SmartAction { .. } => {}
        Action::Keystroke(_) | Action::MouseButton { .. } => tap(a),
        Action::Media { key } => platform::media_key(*key),
        Action::TypeText { text } => platform::type_text(text),
        // Never pass something that looks like a flag to the OS opener.
        Action::LaunchApp { path } if !path.starts_with('-') => platform::open(path),
        Action::OpenUrl { url } if !url.starts_with('-') && url.contains(':') => platform::open(url),
        Action::LaunchApp { .. } | Action::OpenUrl { .. } => {}
        Action::Shell { command } => platform::run_shell(command),
        Action::System { action } => {
            platform::system_action(*action);
        }
    }
}

/// Run a Smart Action's steps in order on a background thread.
pub fn run_sequence(steps: Vec<Step>) {
    std::thread::spawn(move || {
        for step in steps.into_iter().take(MAX_STEPS) {
            match step {
                Step::Delay { ms } => std::thread::sleep(Duration::from_millis(ms.min(MAX_STEP_DELAY_MS) as u64)),
                Step::Action { action } if action.allowed_in_sequence() => {
                    tap(&action);
                    // Give the target app a moment to react between steps.
                    std::thread::sleep(Duration::from_millis(30));
                }
                Step::Action { .. } => {}
            }
        }
    });
}

/// What a control does when nothing is bound — used when a control is diverted
/// for one app but should keep its native behaviour in others.
pub fn native_action(cid: u16) -> Action {
    match cid {
        0x52 => Action::MouseButton { button: 2 },
        0x53 => Action::MouseButton { button: 3 },
        0x56 => Action::MouseButton { button: 4 },
        _ => Action::Disabled,
    }
}
