//! Import button assignments from Logi Options+.
//!
//! Options+ stores everything as one JSON document in
//! `~/Library/Application Support/LogiOptionsPlus/settings.db`
//! (`SELECT file FROM data`). Profiles live under `profile-<uuid>` keys, each
//! with an `applicationId` (the "desktop" profile applies to all apps) and a
//! list of assignments `{slotId: "<slotPrefix>_c<cid>", card: {macro: ...}}`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::action::{Action, MediaKey, Modifier, SystemAction};
use crate::catalog::{cid_from_slot, Catalog};
use crate::config::Config;
use crate::keys::hid_usage_to_code;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("Logi Options+ settings not found at {0}")]
    NotFound(PathBuf),
    #[error("could not read Options+ database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Options+ settings are not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct ImportReport {
    pub imported: usize,
    pub kept_native: usize,
    pub devices: Vec<String>,
    pub apps: Vec<String>,
    pub skipped: Vec<String>,
}

pub fn default_db_path() -> PathBuf {
    crate::paths::options_plus_settings_db().unwrap_or_default()
}

pub fn read_settings_blob(db: &Path) -> Result<Value, ImportError> {
    if !db.exists() {
        return Err(ImportError::NotFound(db.to_path_buf()));
    }
    let conn = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let blob: Vec<u8> = conn.query_row("SELECT file FROM data ORDER BY _id DESC LIMIT 1", [], |r| r.get(0))?;
    Ok(serde_json::from_slice(&blob)?)
}

/// Merge Options+ assignments into `config`. Existing Keymodus bindings for
/// the same control are overwritten; everything else is left alone.
pub fn import(blob: &Value, catalog: Option<&Catalog>, config: &mut Config) -> ImportReport {
    let mut report = ImportReport::default();

    // applicationId -> bundle id
    let mut bundles: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut app_names: BTreeMap<String, String> = BTreeMap::new();
    for a in blob.pointer("/applications/applications").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = a.get("applicationId").and_then(Value::as_str) else { continue };
        let bundle = a
            .get("bundleId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| a.get("applicationPath").and_then(Value::as_str).and_then(|p| bundle_id_of(Path::new(p))));
        bundles.insert(id.to_string(), bundle);
        if let Some(n) = a.get("name").and_then(Value::as_str) {
            app_names.insert(id.to_string(), n.to_string());
        }
    }

    let profile_keys: Vec<String> = blob
        .get("profile_keys")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    for key in profile_keys {
        let Some(profile) = blob.get(&key) else { continue };
        let app_id = profile.get("applicationId").and_then(Value::as_str).unwrap_or_default();
        let is_global = app_names.get(app_id).is_some_and(|n| n == "APPLICATION_NAME_DESKTOP") || !bundles.contains_key(app_id);
        let bundle = if is_global { None } else { bundles.get(app_id).cloned().flatten() };
        if !is_global && bundle.is_none() {
            report.skipped.push(format!("Profile for {} (no bundle id)", app_names.get(app_id).map(String::as_str).unwrap_or(app_id)));
            continue;
        }
        if let Some(b) = &bundle {
            report.apps.push(app_names.get(app_id).cloned().unwrap_or_else(|| b.clone()));
        }

        for a in profile.get("assignments").and_then(Value::as_array).into_iter().flatten() {
            let Some(slot) = a.get("slotId").and_then(Value::as_str) else { continue };
            let Some(cid) = cid_from_slot(slot) else { continue };
            let Some(pid) = pid_from_slot(slot) else { continue };
            let card = a.get("card").cloned().unwrap_or(Value::Null);
            let slot_name = catalog.map(|c| c.slot_names(pid)).unwrap_or_default().into_iter().find(|(c, _)| *c == cid).map(|(_, n)| n);

            let action = match card_to_action(&card) {
                Some(a) => a,
                None => {
                    let what = card.get("name").and_then(Value::as_str).unwrap_or("unknown action");
                    report.skipped.push(format!("{slot}: {what}"));
                    continue;
                }
            };
            if is_native(slot_name.as_deref(), cid, &action) {
                report.kept_native += 1;
                continue;
            }

            let device_key = format!("046d:{pid:04x}");
            let dev = config.devices.entry(device_key.clone()).or_default();
            if dev.name.is_empty() {
                dev.name = catalog.and_then(|c| c.display_name(pid)).unwrap_or_else(|| device_key.clone());
            }
            match &bundle {
                None => dev.bindings.insert(cid, action),
                Some(b) => dev.app_bindings.entry(b.clone()).or_default().insert(cid, action),
            };
            if !report.devices.contains(&dev.name) {
                report.devices.push(dev.name.clone());
            }
            report.imported += 1;
        }
    }
    report
}

/// "mx-vertical-mini-2b031_c253" -> 0xB031
fn pid_from_slot(slot: &str) -> Option<u16> {
    let prefix = slot.rsplit_once('_')?.0;
    let model = prefix.rsplit('-').next()?;
    if model.len() < 4 {
        return None;
    }
    u16::from_str_radix(&model[model.len() - 4..], 16).ok()
}

/// Assignments that just restate what the control does on its own. Importing
/// them would divert the control for nothing, so we leave it native.
fn is_native(slot_name: Option<&str>, cid: u16, action: &Action) -> bool {
    use Action::*;
    match (slot_name, action) {
        (_, MouseButton { button: 2 }) if cid == 0x52 => true,
        (_, MouseButton { button: 3 }) if cid == 0x53 => true,
        (_, MouseButton { button: 4 }) if cid == 0x56 => true,
        (Some("SLOT_NAME_MUTE"), Media { key: MediaKey::Mute }) => true,
        (Some("SLOT_NAME_VOLUME_UP"), Media { key: MediaKey::VolumeUp }) => true,
        (Some("SLOT_NAME_VOLUME_DOWN"), Media { key: MediaKey::VolumeDown }) => true,
        (Some("SLOT_NAME_PLAY_PAUSE"), Media { key: MediaKey::PlayPause }) => true,
        (Some("SLOT_NAME_NEXT_TRACK"), Media { key: MediaKey::Next }) => true,
        (Some("SLOT_NAME_PREVIOUS_TRACK"), Media { key: MediaKey::Previous }) => true,
        // The keyboard dims its own backlight in firmware.
        (Some("SLOT_NAME_BACKLIGHT_UP"), Media { key: MediaKey::KeyboardBrightnessUp }) => true,
        (Some("SLOT_NAME_BACKLIGHT_DOWN"), Media { key: MediaKey::KeyboardBrightnessDown }) => true,
        (Some(n), Keystroke(k)) if n.starts_with("SLOT_NAME_") && k.mods.is_empty() => {
            matches!(
                (n, k.key.as_str()),
                ("SLOT_NAME_HOME", "Home") | ("SLOT_NAME_END", "End") | ("SLOT_NAME_PAGE_UP", "PageUp") | ("SLOT_NAME_PAGE_DOWN", "PageDown")
            )
        }
        _ => false,
    }
}

pub fn card_to_action(card: &Value) -> Option<Action> {
    if card.get("changePointerSpeed").is_some() {
        return Some(Action::CycleDpi);
    }
    let m = card.get("macro")?;
    let ty = m.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "KEYSTROKE" => {
            let k = m.get("keystroke")?;
            let code = hid_usage_to_code(k.get("code")?.as_u64()? as u8)?;
            let mut mods: Vec<Modifier> = k
                .get("modifiers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|v| match v.as_u64()? {
                    224 | 228 => Some(Modifier::Ctrl),
                    225 | 229 => Some(Modifier::Shift),
                    226 | 230 => Some(Modifier::Option),
                    227 | 231 => Some(Modifier::Cmd),
                    _ => None,
                })
                .collect();
            mods.sort();
            mods.dedup();
            Some(Action::keystroke(code, &mods))
        }
        "MEDIA" => {
            let key = match m.pointer("/media/usage")?.as_str()? {
                "PLAY_PAUSE" => MediaKey::PlayPause,
                "NEXT_TRACK" | "SCAN_NEXT_TRACK" | "NEXT" => MediaKey::Next,
                "PREVIOUS_TRACK" | "SCAN_PREVIOUS_TRACK" | "PREVIOUS" => MediaKey::Previous,
                "VOLUME_UP" => MediaKey::VolumeUp,
                "VOLUME_DOWN" => MediaKey::VolumeDown,
                "MUTE" => MediaKey::Mute,
                _ => return None,
            };
            Some(Action::Media { key })
        }
        "MOUSE" => match m.pointer("/mouse/action")?.as_str()? {
            "BUTTON" => {
                let usage = m.pointer("/mouse/hidUsage")?.as_u64()?;
                Some(Action::MouseButton { button: usage.checked_sub(1)? as u8 })
            }
            "OSX_GESTURE_BACK" => Some(Action::System { action: SystemAction::NavigateBack }),
            "OSX_GESTURE_FORWARD" => Some(Action::System { action: SystemAction::NavigateForward }),
            _ => None,
        },
        "SYSTEM" => {
            let a = m.pointer("/system/action")?.as_str()?;
            Some(match a {
                "BACKLIGHT_UP" => Action::Media { key: MediaKey::KeyboardBrightnessUp },
                "BACKLIGHT_DOWN" => Action::Media { key: MediaKey::KeyboardBrightnessDown },
                "BRIGHTNESS_UP" | "SCREEN_BRIGHTNESS_UP" => Action::Media { key: MediaKey::BrightnessUp },
                "BRIGHTNESS_DOWN" | "SCREEN_BRIGHTNESS_DOWN" => Action::Media { key: MediaKey::BrightnessDown },
                "MISSION_CONTROL" => Action::System { action: SystemAction::MissionControl },
                "APP_EXPOSE" => Action::System { action: SystemAction::AppExpose },
                "SHOW_DESKTOP" => Action::System { action: SystemAction::ShowDesktop },
                "LAUNCHPAD" => Action::System { action: SystemAction::Launchpad },
                "LOCK_SCREEN" | "LOCK" => Action::System { action: SystemAction::LockScreen },
                _ => return None,
            })
        }
        "AUDIO" => match m.pointer("/audio/action")?.as_str()? {
            "MUTE_MICROPHONE" => Some(Action::System { action: SystemAction::ToggleMicrophone }),
            _ => None,
        },
        "SCREEN_CAPTURE" => Some(Action::System { action: SystemAction::ScreenshotRegion }),
        _ => None,
    }
}

/// CFBundleIdentifier from an .app's Info.plist (binary or XML).
pub fn bundle_id_of(app: &Path) -> Option<String> {
    let v = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    v.as_dictionary()?.get("CFBundleIdentifier")?.as_string().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn blob() -> Value {
        json!({
          "applications": {"applications": [
            {"applicationId": "desk", "name": "APPLICATION_NAME_DESKTOP"},
            {"applicationId": "application_id_apple_safari", "bundleId": "com.apple.Safari", "name": "Safari"}
          ]},
          "profile_keys": ["profile-desk", "profile-safari"],
          "profile-desk": {"applicationId": "desk", "assignments": [
            {"slotId": "mx-vertical-mini-2b031_c82", "card": {"name": "MB3", "macro": {"type": "MOUSE", "mouse": {"action": "BUTTON", "hidUsage": 3}}}},
            {"slotId": "mx-vertical-mini-2b031_c83", "card": {"macro": {"type": "KEYSTROKE", "keystroke": {"code": 47, "modifiers": [227]}}}},
            {"slotId": "mx-vertical-mini-2b031_c253", "card": {"changePointerSpeed": {}, "macro": {}}},
            {"slotId": "mx-vertical-mini-2b031_mouse_settings", "card": {}},
            {"slotId": "radial-menu-virtual-device-10000000_c415", "card": {"name": "FOLDER", "attribute": "FOLDER"}}
          ]},
          "profile-safari": {"applicationId": "application_id_apple_safari", "assignments": [
            {"slotId": "mx-vertical-mini-2b031_c86", "card": {"macro": {"type": "MEDIA", "media": {"usage": "PLAY_PAUSE"}}}}
          ]}
        })
    }

    #[test]
    fn imports_global_and_app_profiles() {
        let mut c = Config::default();
        let r = import(&blob(), None, &mut c);
        let d = &c.devices["046d:b031"];
        assert_eq!(d.bindings.get(&0x53), Some(&Action::keystroke("BracketLeft", &[Modifier::Cmd])));
        assert_eq!(d.bindings.get(&253), Some(&Action::CycleDpi));
        assert!(!d.bindings.contains_key(&0x52), "native middle click is not diverted");
        assert_eq!(d.app_bindings["com.apple.Safari"][&0x56], Action::Media { key: MediaKey::PlayPause });
        assert_eq!(r.imported, 3);
        assert_eq!(r.kept_native, 1);
        assert_eq!(r.apps, vec!["Safari"]);
        assert_eq!(r.skipped.len(), 1);
    }

    #[test]
    fn slot_pid() {
        assert_eq!(pid_from_slot("mx-mechanical-mini-mac-2b36d_c212"), Some(0xB36D));
    }
}
