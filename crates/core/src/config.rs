//! The user's configuration. Changed only by explicit user actions — never by
//! device events, reconnects or errors. That rule is what keeps bindings from
//! "disappearing".

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::action::Action;

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    /// Keyed by stable device id, e.g. `"046d:b031"`.
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceConfig>,
    #[serde(default)]
    pub generic: GenericConfig,
    #[serde(default)]
    pub settings: AppSettings,
    /// Saved multi-step actions, keyed by a stable id. Buttons refer to them
    /// with [`Action::SmartAction`].
    #[serde(default)]
    pub smart_actions: BTreeMap<String, SmartAction>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: SCHEMA_VERSION,
            devices: BTreeMap::new(),
            generic: GenericConfig::default(),
            settings: AppSettings::default(),
            smart_actions: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Bring an older file up to the current schema. Purely additive: every
    /// new field has a serde default, so this only records the new version.
    pub fn migrate(&mut self) -> bool {
        if self.version >= SCHEMA_VERSION {
            return false;
        }
        self.version = SCHEMA_VERSION;
        true
    }
}

/// A named sequence of steps run in order when a button fires it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SmartAction {
    pub name: String,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// Perform an action (keystrokes are pressed and released).
    Action { action: Action },
    /// Wait before the next step. Clamped to 10 s when run.
    Delay { ms: u32 },
}

pub const MAX_STEP_DELAY_MS: u32 = 10_000;
pub const MAX_STEPS: usize = 32;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Last known name, so offline devices still show up nicely.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    /// Control id (CID) -> action, for all apps.
    #[serde(default)]
    pub bindings: BTreeMap<u16, Action>,
    /// Bundle id -> (CID -> action). Overrides `bindings` while that app is frontmost.
    #[serde(default)]
    pub app_bindings: BTreeMap<String, BTreeMap<u16, Action>>,
    #[serde(default)]
    pub pointer: PointerConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fn_inverted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlight: Option<BacklightConfig>,
    /// Bitmask of keys switched off in hardware (HID++ 0x4521):
    /// 1 Caps Lock, 2 Num Lock, 4 Scroll Lock, 8 Insert, 16 Windows/⌘.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_keys: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BacklightConfig {
    pub enabled: bool,
    /// "auto" follows the ambient light sensor; "manual" uses `level`.
    #[serde(default = "auto_mode")]
    pub mode: String,
    #[serde(default)]
    pub level: u8,
}

fn auto_mode() -> String {
    "auto".into()
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PointerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<u16>,
    /// Values the "cycle pointer speed" action steps through.
    #[serde(default)]
    pub dpi_presets: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hires_scroll: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invert_wheel: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smartshift_ratchet: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smartshift_threshold: Option<u8>,
}

/// Bindings for any mouse or keyboard, through the OS event stream.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GenericConfig {
    /// macOS mouse button number (2 = middle, 3 = back, 4 = forward, ...) -> action.
    #[serde(default)]
    pub mouse_buttons: BTreeMap<u8, Action>,
    /// `KeyboardEvent.code` -> action, for keys pressed without modifiers.
    #[serde(default)]
    pub keys: BTreeMap<String, Action>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "yes")]
    pub launch_at_login: bool,
    /// Check GitHub Releases for a new version on launch and every few hours.
    #[serde(default = "yes")]
    pub auto_update_check: bool,
    /// Pixels of travel before a held gesture button counts as a swipe.
    #[serde(default = "default_gesture_threshold")]
    pub gesture_threshold: u16,
}

fn yes() -> bool {
    true
}
fn default_gesture_threshold() -> u16 {
    60
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings { launch_at_login: true, auto_update_check: true, gesture_threshold: default_gesture_threshold() }
    }
}

impl DeviceConfig {
    /// The action for a control, taking the frontmost app into account.
    pub fn resolve(&self, cid: u16, bundle_id: Option<&str>) -> Action {
        if let Some(app) = bundle_id.and_then(|b| self.app_bindings.get(b)) {
            if let Some(a) = app.get(&cid) {
                return a.clone();
            }
        }
        self.bindings.get(&cid).cloned().unwrap_or_default()
    }

    /// Controls that must be diverted: anything bound to non-default in any scope.
    /// Returns (cid, needs_raw_xy).
    pub fn diverted_controls(&self) -> BTreeMap<u16, bool> {
        let mut out = BTreeMap::new();
        let all = self.bindings.iter().chain(self.app_bindings.values().flat_map(|m| m.iter()));
        for (cid, action) in all {
            if !action.is_default() {
                let e = out.entry(*cid).or_insert(false);
                *e |= action.needs_raw_xy();
            }
        }
        out
    }

    pub fn binding_count(&self) -> usize {
        self.bindings.values().filter(|a| !a.is_default()).count()
            + self.app_bindings.values().flat_map(|m| m.values()).filter(|a| !a.is_default()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, Modifier};

    #[test]
    fn app_override_wins_and_falls_back() {
        let mut d = DeviceConfig::default();
        d.bindings.insert(0x53, Action::keystroke("BracketLeft", &[Modifier::Cmd]));
        let mut safari = BTreeMap::new();
        safari.insert(0x53, Action::keystroke("KeyW", &[Modifier::Cmd]));
        d.app_bindings.insert("com.apple.Safari".into(), safari);

        assert_eq!(d.resolve(0x53, Some("com.apple.Safari")), Action::keystroke("KeyW", &[Modifier::Cmd]));
        assert_eq!(d.resolve(0x53, Some("com.apple.finder")), Action::keystroke("BracketLeft", &[Modifier::Cmd]));
        assert_eq!(d.resolve(0x56, None), Action::Default);
        assert_eq!(d.diverted_controls().into_iter().collect::<Vec<_>>(), vec![(0x53, false)]);
    }

    #[test]
    fn migrates_v1_files() {
        let v1 = r#"{"version":1,"devices":{"046d:b031":{"name":"LIFT","kind":"mouse","bindings":{"83":{"type":"cycle_dpi"}}}}}"#;
        let mut c: Config = serde_json::from_str(v1).unwrap();
        assert!(c.migrate());
        assert_eq!(c.version, SCHEMA_VERSION);
        assert!(c.smart_actions.is_empty());
        assert!(c.settings.auto_update_check);
        assert_eq!(c.devices["046d:b031"].bindings[&83], Action::CycleDpi);
        assert!(!c.migrate());
    }

    #[test]
    fn smart_actions_round_trip() {
        let mut c = Config::default();
        c.smart_actions.insert(
            "s1".into(),
            SmartAction {
                name: "Copy & search".into(),
                steps: vec![
                    Step::Action { action: Action::keystroke("KeyC", &[Modifier::Cmd]) },
                    Step::Delay { ms: 120 },
                    Step::Action { action: Action::OpenUrl { url: "https://example.com".into() } },
                ],
            },
        );
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains(r#""kind":"delay""#));
        assert_eq!(serde_json::from_str::<Config>(&s).unwrap(), c);
    }

    #[test]
    fn round_trips_json() {
        let mut c = Config::default();
        let mut d = DeviceConfig { name: "MX Anywhere".into(), ..Default::default() };
        d.bindings.insert(0xC3, Action::Media { key: crate::action::MediaKey::PlayPause });
        c.devices.insert("046d:b031".into(), d);
        let s = serde_json::to_string_pretty(&c).unwrap();
        assert!(s.contains("\"195\"")); // CIDs are object keys
        let back: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
    }
}
