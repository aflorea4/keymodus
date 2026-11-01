#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod engine;
mod platform;
mod updates;

use std::sync::Arc;
use std::time::Duration;

use engine::Engine;
use logi_core::{Action, BacklightConfig, Config, PointerConfig, SmartAction, Step};
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State, WindowEvent};

type Eng<'a> = State<'a, Arc<Engine>>;

#[derive(Serialize)]
struct Snapshot {
    devices: Vec<engine::DeviceView>,
    config: Config,
    permissions: platform::Permissions,
    capabilities: platform::Capabilities,
    options_plus: platform::OptionsPlus,
    guard_log: Vec<engine::GuardEntry>,
    notices: Vec<engine::Notice>,
    launch_at_login: bool,
    backups: Vec<String>,
    config_dir: String,
    update: Option<updates::UpdateInfo>,
    version: &'static str,
}

#[tauri::command]
fn get_state(engine: Eng, pending: State<updates::Pending>) -> Snapshot {
    Snapshot {
        devices: engine.devices(),
        config: engine.config.read().unwrap().clone(),
        permissions: platform::permissions(),
        capabilities: platform::capabilities(),
        options_plus: platform::options_plus(),
        guard_log: engine.guard_log(),
        notices: engine.notices.lock().unwrap().clone(),
        launch_at_login: platform::launch_at_login(),
        config_dir: engine.store.dir().to_string_lossy().into_owned(),
        update: pending.info(),
        backups: engine.store.backups().unwrap_or_default().iter().filter_map(|p| p.file_name().map(|f| f.to_string_lossy().into_owned())).collect(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

/// `app` = bundle id for a per-app override, or null for all apps.
#[tauri::command]
fn set_binding(engine: Eng, device: String, cid: u16, app: Option<String>, action: Action) -> Result<Config, String> {
    engine.mutate(Some(&device), |c| {
        let d = c.devices.entry(device.clone()).or_default();
        match &app {
            None => {
                if action.is_default() {
                    d.bindings.remove(&cid);
                } else {
                    d.bindings.insert(cid, action.clone());
                }
            }
            Some(bundle) => {
                let m = d.app_bindings.entry(bundle.clone()).or_default();
                m.insert(cid, action.clone());
            }
        }
    })?;
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn clear_app_binding(engine: Eng, device: String, cid: u16, app: String) -> Result<Config, String> {
    engine.mutate(Some(&device), |c| {
        if let Some(d) = c.devices.get_mut(&device) {
            if let Some(m) = d.app_bindings.get_mut(&app) {
                m.remove(&cid);
                if m.is_empty() {
                    d.app_bindings.remove(&app);
                }
            }
        }
    })?;
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn remove_app_profile(engine: Eng, device: String, app: String) -> Result<Config, String> {
    engine.mutate(Some(&device), |c| {
        if let Some(d) = c.devices.get_mut(&device) {
            d.app_bindings.remove(&app);
        }
    })?;
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn set_pointer(engine: Eng, device: String, pointer: PointerConfig) -> Result<Config, String> {
    engine.mutate(Some(&device), |c| c.devices.entry(device.clone()).or_default().pointer = pointer.clone())?;
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn set_fn_inverted(engine: Eng, device: String, on: bool) -> Result<Config, String> {
    engine.mutate(Some(&device), |c| c.devices.entry(device.clone()).or_default().fn_inverted = Some(on))?;
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn set_backlight(engine: Eng, device: String, backlight: BacklightConfig) -> Result<Config, String> {
    engine.mutate(Some(&device), |c| c.devices.entry(device.clone()).or_default().backlight = Some(backlight.clone()))?;
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn set_disabled_keys(engine: Eng, device: String, mask: u8) -> Result<Config, String> {
    engine.mutate(Some(&device), |c| c.devices.entry(device.clone()).or_default().disabled_keys = Some(mask))?;
    Ok(engine.config.read().unwrap().clone())
}

/// Move the device to another Easy-Switch channel (0-based).
#[tauri::command]
fn switch_host(engine: Eng, device: String, host: u8) -> Result<(), String> {
    if host > 5 {
        return Err("invalid channel".into());
    }
    engine.switch_host(&device, host);
    Ok(())
}

/// Clear every binding and app profile for one device; hardware settings stay.
#[tauri::command]
fn reset_device(engine: Eng, device: String) -> Result<Config, String> {
    engine.mutate(Some(&device), |c| {
        if let Some(d) = c.devices.get_mut(&device) {
            d.bindings.clear();
            d.app_bindings.clear();
        }
    })?;
    Ok(engine.config.read().unwrap().clone())
}

fn sanitize_steps(steps: Vec<Step>) -> Vec<Step> {
    steps
        .into_iter()
        .filter(|s| match s {
            Step::Action { action } => action.allowed_in_sequence(),
            Step::Delay { .. } => true,
        })
        .map(|s| match s {
            Step::Delay { ms } => Step::Delay { ms: ms.min(logi_core::config::MAX_STEP_DELAY_MS) },
            other => other,
        })
        .take(logi_core::config::MAX_STEPS)
        .collect()
}

/// Create (id = null) or update a Smart Action. Returns the config.
#[tauri::command]
fn save_smart_action(engine: Eng, id: Option<String>, action: SmartAction) -> Result<Config, String> {
    let name = action.name.trim().chars().take(60).collect::<String>();
    let action = SmartAction { name: if name.is_empty() { "Smart Action".into() } else { name }, steps: sanitize_steps(action.steps) };
    engine.mutate(None, |c| {
        let id = id.clone().filter(|i| c.smart_actions.contains_key(i)).unwrap_or_else(|| format!("sa{}", engine::now_ms()));
        c.smart_actions.insert(id, action.clone());
    })?;
    Ok(engine.config.read().unwrap().clone())
}

/// Delete a Smart Action and reset any button that used it.
#[tauri::command]
fn delete_smart_action(engine: Eng, id: String) -> Result<Config, String> {
    let uses = |a: &Action| matches!(a, Action::SmartAction { id: x } if *x == id);
    engine.mutate(None, |c| {
        c.smart_actions.remove(&id);
        for d in c.devices.values_mut() {
            d.bindings.retain(|_, a| !uses(a));
            for m in d.app_bindings.values_mut() {
                m.retain(|_, a| !uses(a));
            }
        }
        c.generic.mouse_buttons.retain(|_, a| !uses(a));
        c.generic.keys.retain(|_, a| !uses(a));
    })?;
    engine.reapply_all();
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn test_steps(engine: Eng, steps: Vec<Step>) {
    engine.test_steps(sanitize_steps(steps));
}

/// Write the configuration to the Downloads folder and reveal it.
#[tauri::command]
fn export_config(engine: Eng) -> Result<String, String> {
    let cfg = engine.config.read().unwrap().clone();
    let data = serde_json::to_vec_pretty(&cfg).map_err(|e| e.to_string())?;
    let dir = logi_core::paths::export_dir();
    let path = dir.join(format!("keymodus-settings-{}.json", engine::now_ms() / 1000));
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    platform::reveal_file(&path);
    Ok(path.to_string_lossy().into_owned())
}

/// Replace the configuration with an exported file. The current one is kept as a backup.
#[tauri::command]
fn import_config(engine: Eng, json: String) -> Result<Config, String> {
    if json.len() > 4 * 1024 * 1024 {
        return Err("That file is too large to be a Keymodus settings file.".into());
    }
    let mut c: Config = serde_json::from_str(&json).map_err(|e| format!("Not a Keymodus settings file: {e}"))?;
    c.migrate();
    engine.store.save(&c).map_err(|e| e.to_string())?;
    engine.replace_config(c.clone());
    Ok(c)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum GenericKind {
    Mouse,
    Key,
}

#[tauri::command]
fn set_generic_binding(engine: Eng, kind: GenericKind, code: String, action: Action) -> Result<Config, String> {
    engine.mutate(None, |c| match kind {
        GenericKind::Mouse => {
            if let Ok(b) = code.parse::<u8>() {
                if action.is_default() {
                    c.generic.mouse_buttons.remove(&b);
                } else {
                    c.generic.mouse_buttons.insert(b, action.clone());
                }
            }
        }
        GenericKind::Key => {
            if action.is_default() {
                c.generic.keys.remove(&code);
            } else {
                c.generic.keys.insert(code.clone(), action.clone());
            }
        }
    })?;
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn forget_device(engine: Eng, device: String) -> Result<Config, String> {
    engine.mutate(None, |c| {
        c.devices.remove(&device);
    })?;
    engine.publish();
    Ok(engine.config.read().unwrap().clone())
}

#[tauri::command]
fn set_gesture_threshold(engine: Eng, px: u16) -> Result<(), String> {
    engine.mutate(None, |c| c.settings.gesture_threshold = px.clamp(20, 300))
}

#[tauri::command]
fn test_action(engine: Eng, action: Action) {
    engine.inner().test_action(&action);
}

#[derive(Serialize)]
struct ImportResult {
    report: logi_core::import::ImportReport,
    config: Config,
}

#[tauri::command]
fn import_options_plus(engine: Eng) -> Result<ImportResult, String> {
    if !platform::capabilities().options_plus {
        return Err("Importing from Options+ is only supported on macOS for now.".into());
    }
    let blob = logi_core::import::read_settings_blob(&logi_core::import::default_db_path()).map_err(|e| e.to_string())?;
    let catalog = logi_core::catalog::Catalog::open();
    let mut report = None;
    engine.mutate(None, |c| report = Some(logi_core::import::import(&blob, catalog.as_ref(), c)))?;
    engine.reapply_all();
    Ok(ImportResult { report: report.unwrap_or_default(), config: engine.config.read().unwrap().clone() })
}

#[tauri::command]
fn restore_backup(engine: Eng, name: String) -> Result<Config, String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("invalid backup name".into());
    }
    let path = engine.store.backup_dir().join(&name);
    let c = engine.store.restore(&path).map_err(|e| e.to_string())?;
    engine.replace_config(c.clone());
    Ok(c)
}

#[tauri::command]
async fn list_apps() -> Vec<platform::AppInfo> {
    tauri::async_runtime::spawn_blocking(platform::installed_apps).await.unwrap_or_default()
}

#[tauri::command]
fn request_permission(kind: String) -> bool {
    platform::request_permission(&kind)
}

#[tauri::command]
fn open_settings(pane: String) {
    platform::open_settings(&pane);
}

#[tauri::command]
fn pause_options_plus(engine: Eng) -> Result<(), String> {
    platform::pause_options_plus()?;
    engine.log("Keymodus", "info", "Logi Options+ paused for this session");
    std::thread::sleep(Duration::from_millis(300));
    engine.reapply_all();
    Ok(())
}

#[tauri::command]
fn resume_options_plus() -> Result<(), String> {
    platform::resume_options_plus()
}

#[tauri::command]
fn set_launch_at_login(engine: Eng, on: bool) -> Result<(), String> {
    platform::set_launch_at_login(on)?;
    engine.mutate(None, |c| c.settings.launch_at_login = on)
}

#[tauri::command]
fn set_auto_update(engine: Eng, on: bool) -> Result<(), String> {
    engine.mutate(None, |c| c.settings.auto_update_check = on)
}

#[tauri::command]
fn dismiss_notice(engine: Eng, id: String) {
    engine.notices.lock().unwrap().retain(|n| n.id != id);
}

#[tauri::command]
fn reveal_logs(engine: Eng) {
    platform::reveal(&engine.log_dir());
}

/// Open a page of the project's GitHub repository (fixed origin).
#[tauri::command]
fn open_repo(path: String) {
    let path: String = path.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.')).collect();
    platform::open(&format!("https://github.com/aflorea4/keymodus/{}", path.trim_start_matches('/')));
}

#[tauri::command]
fn reveal_config(engine: Eng) {
    platform::reveal(engine.store.dir());
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    let engine = Engine::new();
    let background = std::env::args().any(|a| a == "--background");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(engine.clone())
        .manage(updates::Pending::default())
        .register_uri_scheme_protocol("devart", |ctx, req| {
            let engine = ctx.app_handle().state::<Arc<Engine>>();
            let id = req.uri().path().trim_start_matches('/').to_string();
            let file = engine.art_files.lock().unwrap().get(&id).cloned();
            match file.and_then(|p| std::fs::read(p).ok()) {
                Some(bytes) => tauri::http::Response::builder()
                    .header("Content-Type", "image/webp")
                    .header("Cache-Control", "max-age=86400")
                    .body(bytes)
                    .unwrap(),
                None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_binding,
            clear_app_binding,
            remove_app_profile,
            set_pointer,
            set_fn_inverted,
            set_backlight,
            set_disabled_keys,
            switch_host,
            reset_device,
            save_smart_action,
            delete_smart_action,
            test_steps,
            export_config,
            import_config,
            set_auto_update,
            reveal_config,
            open_repo,
            updates::check_update,
            updates::install_update,
            set_generic_binding,
            forget_device,
            set_gesture_threshold,
            test_action,
            import_options_plus,
            restore_backup,
            list_apps,
            request_permission,
            open_settings,
            pause_options_plus,
            resume_options_plus,
            set_launch_at_login,
            dismiss_notice,
            reveal_logs,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            engine.set_emitter(Box::new(move |event, payload| {
                let _ = handle.emit(event, payload);
            }));
            engine.start();
            platform::refresh_login_item();
            updates::start_background_checks(app.handle().clone(), engine.clone());

            // Generic mice / keyboards through an OS input hook; retry until
            // the permission it needs is granted (where supported at all).
            let tap_engine = engine.clone();
            std::thread::spawn(move || loop {
                if !platform::capabilities().any_device {
                    break;
                }
                if platform::event_tap_ready() {
                    let e = tap_engine.clone();
                    if platform::start_event_tap(Box::new(move |ev| e.on_tap_event(ev))) {
                        tap_engine.log("Keymodus", "info", "Watching all mice and keyboards");
                        break;
                    }
                }
                std::thread::sleep(Duration::from_secs(3));
            });

            // Menus: closing the window must not stop the bindings.
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::Submenu;
                let quit = MenuItem::with_id(app, "quit", "Quit Keymodus", true, None::<&str>)?;
                let app_menu = Submenu::with_items(
                    app,
                    "Keymodus",
                    true,
                    &[&PredefinedMenuItem::about(app, Some("About Keymodus"), None)?, &PredefinedMenuItem::separator(app)?, &quit],
                )?;
                let edit = Submenu::with_items(
                    app,
                    "Edit",
                    true,
                    &[
                        &PredefinedMenuItem::undo(app, None)?,
                        &PredefinedMenuItem::redo(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::cut(app, None)?,
                        &PredefinedMenuItem::copy(app, None)?,
                        &PredefinedMenuItem::paste(app, None)?,
                        &PredefinedMenuItem::select_all(app, None)?,
                    ],
                )?;
                // No ⌘W / ⌘H / ⌘M accelerators here: they would fire while the user is
                // recording a shortcut. The page handles ⌘W itself when not recording.
                app.set_menu(Menu::with_items(app, &[&app_menu, &edit])?)?;
            }

            let open = MenuItem::with_id(app, "open", "Open Keymodus", true, None::<&str>)?;
            let tray_quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &tray_quit])?;
            let mut tray = TrayIconBuilder::with_id("main").menu(&tray_menu).show_menu_on_left_click(true).tooltip("Keymodus");
            // Monochrome template icon, so macOS tints it for light and dark menu
            // bars; the coloured app icon elsewhere.
            if cfg!(target_os = "macos") {
                tray = tray.icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?).icon_as_template(true);
            } else if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            app.on_menu_event(|app, e| match e.id().as_ref() {
                "quit" => app.exit(0),
                "open" => show_main(app),
                _ => {}
            });

            if background {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Keymodus");

    app.run(|_app, _event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            show_main(_app);
        }
    });
}
