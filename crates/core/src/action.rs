//! What a button can do.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    /// ⌘ on macOS, the Windows key on Windows, Super on Linux.
    Cmd,
    Shift,
    /// ⌥ on macOS, Alt elsewhere.
    Option,
    Ctrl,
    /// macOS only; ignored elsewhere.
    Fn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
    BrightnessUp,
    BrightnessDown,
    KeyboardBrightnessUp,
    KeyboardBrightnessDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAction {
    MissionControl,
    AppExpose,
    ShowDesktop,
    Launchpad,
    Spotlight,
    Screenshot,
    ScreenshotRegion,
    LockScreen,
    EmojiPicker,
    ToggleMicrophone,
    NavigateBack,
    NavigateForward,
    NextSpace,
    PreviousSpace,
    SwitchApp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keystroke {
    /// A `KeyboardEvent.code` value ("KeyA", "Digit1", "F5", "ArrowLeft", ...).
    pub key: String,
    #[serde(default)]
    pub mods: Vec<Modifier>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gesture {
    pub click: Box<Action>,
    pub up: Box<Action>,
    pub down: Box<Action>,
    pub left: Box<Action>,
    pub right: Box<Action>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// The control does what it does out of the box (not diverted).
    #[default]
    Default,
    /// Swallow the press.
    Disabled,
    Keystroke(Keystroke),
    Media {
        key: MediaKey,
    },
    /// Emit a mouse button (0 = left, 1 = right, 2 = middle, 3 = back, 4 = forward ...).
    MouseButton {
        button: u8,
    },
    System {
        action: SystemAction,
    },
    LaunchApp {
        path: String,
    },
    OpenUrl {
        url: String,
    },
    Shell {
        command: String,
    },
    TypeText {
        text: String,
    },
    /// Step through the device's pointer speed presets.
    CycleDpi,
    /// Hold and move: one action per direction, plus a plain click.
    Gesture(Gesture),
    /// Run a saved Smart Action (a sequence of steps) by id.
    SmartAction {
        id: String,
    },
}

impl Action {
    pub fn is_default(&self) -> bool {
        matches!(self, Action::Default)
    }

    pub fn needs_raw_xy(&self) -> bool {
        matches!(self, Action::Gesture(_))
    }

    /// Actions that may appear as a step inside a Smart Action.
    pub fn allowed_in_sequence(&self) -> bool {
        !matches!(self, Action::Gesture(_) | Action::SmartAction { .. } | Action::CycleDpi | Action::Default)
    }

    pub fn keystroke(key: &str, mods: &[Modifier]) -> Action {
        Action::Keystroke(Keystroke { key: key.into(), mods: mods.to_vec() })
    }
}
