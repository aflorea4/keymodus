//! The update agent: checks GitHub Releases for a newer signed build, tells
//! the UI, and installs it on request.
//!
//! The release workflow publishes `latest.json` (version, notes, per-platform
//! download URLs and minisign signatures). The public key in
//! `tauri.conf.json` verifies every download before it is installed, so a
//! tampered or unsigned file is rejected.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::engine::Engine;

const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

/// The update found by the last check, ready to install.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

impl Pending {
    pub fn info(&self) -> Option<UpdateInfo> {
        self.0.lock().unwrap().as_ref().map(info_of)
    }
}

fn info_of(u: &Update) -> UpdateInfo {
    UpdateInfo { version: u.version.clone(), current: u.current_version.clone(), notes: u.body.clone(), date: u.date.map(|d| d.to_string()) }
}

async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let info = update.as_ref().map(info_of);
    if let Some(p) = app.try_state::<Pending>() {
        *p.0.lock().unwrap() = update;
    }
    Ok(info)
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    check(&app).await
}

/// Download, verify and install the pending update, then restart.
#[tauri::command]
pub async fn install_update(app: AppHandle, pending: State<'_, Pending>) -> Result<(), String> {
    let update = pending.0.lock().unwrap().take();
    let update = match update {
        Some(u) => u,
        None => app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?.ok_or("Keymodus is up to date.")?,
    };
    let progress = app.clone();
    let mut done: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                done += chunk as u64;
                let _ = progress.emit("update-progress", serde_json::json!({ "done": done, "total": total }));
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

use tauri::Manager;

/// Check shortly after launch and then every six hours, when enabled.
pub fn start_background_checks(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(async move {
        tokio_sleep(FIRST_CHECK_AFTER).await;
        loop {
            let enabled = engine.config.read().unwrap().settings.auto_update_check;
            if enabled && cfg!(not(debug_assertions)) {
                match check(&app).await {
                    Ok(Some(info)) => {
                        engine.log("Keymodus", "info", format!("Update available: {}", info.version));
                        let _ = app.emit("update", info);
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("update check failed: {e}"),
                }
            }
            tokio_sleep(CHECK_EVERY).await;
        }
    });
}

async fn tokio_sleep(d: Duration) {
    // Avoid a direct tokio dependency: sleep on a blocking thread.
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}
