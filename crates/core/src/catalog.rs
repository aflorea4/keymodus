//! Device artwork and button hotspots, read from a local Logi Options+ install
//! when one is present. Nothing is copied or redistributed: if Options+ is not
//! installed, the UI falls back to its own drawings.
//!
//! Layout on disk (reverse engineered from Options+ 2.x):
//!
//! ```text
//! /Library/Application Support/Logi/LogiOptionsPlus/
//!   devices.json                       catalog: modelId ("2b031") -> depot ("mx_vertical_mini")
//!   depots/<build>/<depot>/manifest.json   modelId[_extN] -> {device_buttons_image, image_metadata}
//!   depots/<build>/<depot>/<metadata>.json  hotspots: slotId "<prefix>_c<cid>", marker x/y in %
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

pub const OPTIONS_PLUS_ROOT: &str = "/Library/Application Support/Logi/LogiOptionsPlus";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hotspot {
    pub cid: u16,
    pub name: String,
    /// Percent of image width / height.
    pub x: f32,
    pub y: f32,
    /// Which side the label sits on: -1 left, 0 centre, 1 right.
    pub side: i8,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DeviceArt {
    pub display_name: String,
    pub kind: String,
    pub image: PathBuf,
    pub width: u32,
    pub height: u32,
    pub hotspots: Vec<Hotspot>,
}

pub struct Catalog {
    root: PathBuf,
    devices: Vec<Value>,
}

impl Catalog {
    pub fn open() -> Option<Catalog> {
        Self::open_at(&crate::paths::options_plus_root()?)
    }

    pub fn open_at(root: &Path) -> Option<Catalog> {
        let v: Value = serde_json::from_slice(&fs::read(root.join("devices.json")).ok()?).ok()?;
        let devices = v.get("devices")?.as_array()?.clone();
        Some(Catalog { root: root.to_path_buf(), devices })
    }

    /// Catalog entry whose model id ends with this product id (Options+ prefixes
    /// the transport: "2b031" is Bluetooth PID 0xB031).
    fn entry(&self, pid: u16) -> Option<&Value> {
        let suffix = format!("{pid:04x}");
        self.devices.iter().find(|d| d.get("modelId").and_then(Value::as_str).is_some_and(|m| m.to_ascii_lowercase().ends_with(&suffix)))
    }

    pub fn display_name(&self, pid: u16) -> Option<String> {
        self.entry(pid)?.get("displayName")?.as_str().map(str::to_string)
    }

    /// Options+'s slot prefix for this device, e.g. "mx-vertical-mini-2b031".
    pub fn slot_prefix(&self, pid: u16) -> Option<String> {
        self.entry(pid)?.get("slotPrefix")?.as_str().map(str::to_string)
    }

    pub fn art(&self, pid: u16, ext_model: u8) -> Option<DeviceArt> {
        let entry = self.entry(pid)?;
        let model_id = entry.get("modelId")?.as_str()?;
        let depot = entry.get("depot")?.as_str()?;
        let display_name = entry.get("displayName").and_then(Value::as_str).unwrap_or(model_id).to_string();
        let kind = entry.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        let dir = self.depot_dir(depot)?;

        let manifest: Value = serde_json::from_slice(&fs::read(dir.join("manifest.json")).ok()?).ok()?;
        let models = manifest.get("devices")?.as_array()?;
        let want_ext = format!("{model_id}_ext{ext_model}");
        let model = models
            .iter()
            .find(|m| ext_model != 0 && m.get("modelId").and_then(Value::as_str) == Some(want_ext.as_str()))
            .or_else(|| models.iter().find(|m| m.get("modelId").and_then(Value::as_str) == Some(model_id)))?;
        let resource = |key: &str| -> Option<String> {
            model
                .get("resources")?
                .as_array()?
                .iter()
                .find(|r| r.get("key").and_then(Value::as_str) == Some(key))?
                .get("src")?
                .as_str()
                .map(str::to_string)
        };
        let (image_key, image_file) = ["device_buttons_image", "device_keys_image"].iter().find_map(|k| resource(k).map(|f| (*k, f)))?;
        let meta: Value = serde_json::from_slice(&fs::read(dir.join(resource("image_metadata")?)).ok()?).ok()?;
        let image_meta = meta.get("images")?.as_array()?.iter().find(|i| i.get("key").and_then(Value::as_str) == Some(image_key))?;
        let width = image_meta.pointer("/origin/width").and_then(Value::as_u64).unwrap_or(400) as u32;
        let height = image_meta.pointer("/origin/height").and_then(Value::as_u64).unwrap_or(400) as u32;

        let mut hotspots = vec![];
        for a in image_meta.get("assignments").and_then(Value::as_array).into_iter().flatten() {
            let Some(cid) = a.get("slotId").and_then(Value::as_str).and_then(cid_from_slot) else { continue };
            let name = a.get("slotName").and_then(Value::as_str).map(pretty_slot_name).unwrap_or_default();
            let mut x = a.pointer("/marker/x").and_then(Value::as_f64).unwrap_or(50.0) as f32;
            let mut y = a.pointer("/marker/y").and_then(Value::as_f64).unwrap_or(50.0) as f32;
            // Key hotspots are boxes: the marker is their top-left corner and
            // `styles` gives the size in image pixels. Point at the centre.
            if let (Some(w), Some(h)) = (a.pointer("/styles/width").and_then(Value::as_f64), a.pointer("/styles/height").and_then(Value::as_f64)) {
                x += (w / width as f64 * 50.0) as f32;
                y += (h / height as f64 * 50.0) as f32;
            }
            let side = a.pointer("/label/x").and_then(Value::as_i64).unwrap_or(1).signum() as i8;
            hotspots.push(Hotspot { cid, name, x, y, side });
        }
        let image = dir.join(image_file);
        image.exists().then_some(DeviceArt { display_name, kind, image, width, height, hotspots })
    }

    /// Native slot names ("SLOT_NAME_VOLUME_UP") by CID, for the importer.
    pub fn slot_names(&self, pid: u16) -> Vec<(u16, String)> {
        let Some(entry) = self.entry(pid) else { return vec![] };
        let Some(dir) = entry.get("depot").and_then(Value::as_str).and_then(|d| self.depot_dir(d)) else { return vec![] };
        let mut out = vec![];
        for f in ["metadata.json", "core_metadata_right.json", "core_metadata.json"] {
            let Ok(bytes) = fs::read(dir.join(f)) else { continue };
            let Ok(v) = serde_json::from_slice::<Value>(&bytes) else { continue };
            for img in v.get("images").and_then(Value::as_array).into_iter().flatten() {
                for a in img.get("assignments").and_then(Value::as_array).into_iter().flatten() {
                    if let (Some(cid), Some(name)) =
                        (a.get("slotId").and_then(Value::as_str).and_then(cid_from_slot), a.get("slotName").and_then(Value::as_str))
                    {
                        out.push((cid, name.to_string()));
                    }
                }
            }
            break;
        }
        out
    }

    /// Newest build directory that contains this depot.
    fn depot_dir(&self, depot: &str) -> Option<PathBuf> {
        let mut builds: Vec<(u64, PathBuf)> = fs::read_dir(self.root.join("depots"))
            .ok()?
            .filter_map(|e| e.ok())
            .filter_map(|e| Some((e.file_name().to_str()?.parse::<u64>().ok()?, e.path())))
            .collect();
        builds.sort_by_key(|b| std::cmp::Reverse(b.0));
        builds.into_iter().map(|(_, p)| p.join(depot)).find(|p| p.join("manifest.json").exists())
    }
}

/// "mx-vertical-mini-2b031_c253" -> 253
pub fn cid_from_slot(slot: &str) -> Option<u16> {
    let (_, tail) = slot.rsplit_once("_c")?;
    tail.parse().ok()
}

/// "SLOT_NAME_MUTE_UNMUTE_AUDIO" -> "Mute unmute audio", with a few hand fixes.
pub fn pretty_slot_name(raw: &str) -> String {
    let s = raw.trim_start_matches("SLOT_NAME_");
    match s {
        "DPI_BUTTON" => return "Top button".into(),
        "MUTE_UNMUTE_AUDIO" => return "Microphone".into(),
        "MIDDLE_BUTTON" => return "Middle button".into(),
        "BACK_BUTTON" => return "Back button".into(),
        "FORWARD_BUTTON" => return "Forward button".into(),
        "GESTURE_BUTTON" => return "Gesture button".into(),
        "MODESHIFT_BUTTON" => return "Wheel mode button".into(),
        _ => {}
    }
    let lower = s.replace('_', " ").to_lowercase();
    let mut c = lower.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slot_ids() {
        assert_eq!(cid_from_slot("mx-vertical-mini-2b031_c253"), Some(253));
        assert_eq!(cid_from_slot("mx-vertical-mini-2b031_mouse_settings"), None);
        assert_eq!(pretty_slot_name("SLOT_NAME_VOLUME_UP"), "Volume up");
        assert_eq!(pretty_slot_name("SLOT_NAME_DPI_BUTTON"), "Top button");
    }

    /// Runs against the real Options+ install when present.
    #[test]
    fn reads_local_options_plus_when_installed() {
        let Some(c) = Catalog::open() else { return };
        if let Some(art) = c.art(0xB031, 0) {
            assert!(art.hotspots.iter().any(|h| h.cid == 0x53));
            assert!(art.image.exists());
        }
    }
}
