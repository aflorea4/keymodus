//! Per-OS locations for settings, logs and Logi Options+ files.
//!
//! | | macOS | Windows | Linux |
//! |---|---|---|---|
//! | settings | `~/Library/Application Support/Keymodus` | `%APPDATA%\Keymodus` | `$XDG_CONFIG_HOME/keymodus` |
//! | logs | `~/Library/Logs/Keymodus` | `%LOCALAPPDATA%\Keymodus\logs` | `$XDG_STATE_HOME/keymodus` |

use std::path::PathBuf;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Where `config.json` and `backups/` live.
pub fn config_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Keymodus")
    } else if cfg!(windows) {
        dirs::config_dir().unwrap_or_else(home).join("Keymodus")
    } else {
        dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("keymodus")
    }
}

/// The settings folder used by the app's previous name, if any (macOS only).
pub fn legacy_config_dir() -> Option<PathBuf> {
    cfg!(target_os = "macos").then(|| home().join("Library/Application Support/Logidriver"))
}

pub fn log_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Logs/Keymodus")
    } else if cfg!(windows) {
        dirs::data_local_dir().unwrap_or_else(home).join("Keymodus").join("logs")
    } else {
        dirs::state_dir().or_else(dirs::data_local_dir).unwrap_or_else(|| home().join(".local/state")).join("keymodus")
    }
}

/// Logi Options+ settings database (read-only import source). macOS only for now.
pub fn options_plus_settings_db() -> Option<PathBuf> {
    cfg!(target_os = "macos").then(|| home().join("Library/Application Support/LogiOptionsPlus/settings.db"))
}

/// Logi Options+ device catalog (artwork + hotspots). macOS only for now.
pub fn options_plus_root() -> Option<PathBuf> {
    cfg!(target_os = "macos").then(|| PathBuf::from("/Library/Application Support/Logi/LogiOptionsPlus"))
}

/// A writable folder for exports (Downloads, falling back to home).
pub fn export_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(home)
}
