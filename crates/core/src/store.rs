//! Crash-safe configuration storage.
//!
//! Logi Options+ keeps every setting in a single JSON blob inside one SQLite
//! row and rewrites it wholesale; its snapshot table is empty. One bad write
//! or one failed read and everything falls back to defaults. Here instead:
//!
//! * every save goes to a temp file, is fsynced, then atomically renamed;
//! * the previous version is kept as a timestamped backup (last 30);
//! * a config that fails to parse is moved aside, never overwritten, and
//!   the newest readable backup is loaded instead;
//! * if nothing is readable we start empty *and say so* — never silently.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{Config, SCHEMA_VERSION};

const MAX_BACKUPS: usize = 30;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("encode: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// Normal load.
    Loaded,
    /// No config yet (first run).
    Fresh,
    /// Main file was unreadable; restored from this backup. The bad file was kept.
    Recovered { from: PathBuf, quarantined: PathBuf },
    /// Nothing readable at all; started empty. The bad file was kept.
    Unrecoverable { quarantined: PathBuf },
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Store { dir: dir.into() }
    }

    pub fn default_dir() -> PathBuf {
        let dir = crate::paths::config_dir();
        // The app used to be called Logidriver; carry its settings over once.
        if let Some(legacy) = crate::paths::legacy_config_dir() {
            if !dir.exists() && legacy.is_dir() {
                let _ = fs::rename(&legacy, &dir);
            }
        }
        dir
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.dir.join("backups")
    }

    pub fn load(&self) -> Result<(Config, LoadOutcome), StoreError> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path();
        if !path.exists() {
            // A crash between rename steps could leave only the temp file.
            let tmp = self.dir.join("config.json.tmp");
            if let Ok(c) = read_config(&tmp) {
                fs::rename(&tmp, &path)?;
                return Ok((c, LoadOutcome::Loaded));
            }
            return Ok((Config::default(), LoadOutcome::Fresh));
        }
        match read_config(&path) {
            Ok(mut c) => {
                c.migrate();
                Ok((c, LoadOutcome::Loaded))
            }
            Err(_) => {
                let quarantined = self.dir.join(format!("config.corrupt-{}.json", stamp()));
                fs::rename(&path, &quarantined)?;
                for b in self.backups()? {
                    if let Ok(c) = read_config(&b) {
                        self.save(&c)?;
                        return Ok((c, LoadOutcome::Recovered { from: b, quarantined }));
                    }
                }
                Ok((Config::default(), LoadOutcome::Unrecoverable { quarantined }))
            }
        }
    }

    pub fn save(&self, config: &Config) -> Result<(), StoreError> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path();
        let mut c = config.clone();
        c.version = SCHEMA_VERSION;
        let data = serde_json::to_vec_pretty(&c)?;

        if path.exists() {
            let current = fs::read(&path)?;
            if current == data {
                return Ok(());
            }
            fs::create_dir_all(self.backup_dir())?;
            fs::write(self.backup_dir().join(format!("config-{}.json", stamp())), &current)?;
            self.prune_backups()?;
        }

        let tmp = self.dir.join("config.json.tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&data)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        if let Ok(d) = fs::File::open(&self.dir) {
            let _ = d.sync_all();
        }
        Ok(())
    }

    /// Backups, newest first.
    pub fn backups(&self) -> Result<Vec<PathBuf>, StoreError> {
        let dir = self.backup_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut v: Vec<PathBuf> =
            fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        v.sort();
        v.reverse();
        Ok(v)
    }

    pub fn restore(&self, backup: &Path) -> Result<Config, StoreError> {
        let c = read_config(backup)?;
        self.save(&c)?;
        Ok(c)
    }

    fn prune_backups(&self) -> Result<(), StoreError> {
        for old in self.backups()?.into_iter().skip(MAX_BACKUPS) {
            let _ = fs::remove_file(old);
        }
        Ok(())
    }
}

fn read_config(path: &Path) -> Result<Config, StoreError> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn stamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    let fmt = time::macros::format_description!("[year][month][day]-[hour][minute][second]-[subsecond digits:6]");
    now.format(&fmt).unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::config::DeviceConfig;

    fn sample(n: u16) -> Config {
        let mut c = Config::default();
        let mut d = DeviceConfig::default();
        d.bindings.insert(n, Action::Disabled);
        c.devices.insert("046d:b031".into(), d);
        c
    }

    #[test]
    fn saves_and_keeps_backups() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path());
        assert_eq!(s.load().unwrap().1, LoadOutcome::Fresh);
        s.save(&sample(1)).unwrap();
        s.save(&sample(2)).unwrap();
        s.save(&sample(2)).unwrap(); // identical: no extra backup
        assert_eq!(s.backups().unwrap().len(), 1);
        let (c, o) = s.load().unwrap();
        assert_eq!(o, LoadOutcome::Loaded);
        assert_eq!(c, sample(2));
    }

    #[test]
    fn recovers_from_corruption_without_losing_data() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path());
        s.save(&sample(1)).unwrap();
        s.save(&sample(2)).unwrap();
        fs::write(s.path(), b"{\"version\": 1, \"devices\": {tru").unwrap();
        let (c, outcome) = s.load().unwrap();
        assert_eq!(c, sample(1), "newest readable backup wins");
        match outcome {
            LoadOutcome::Recovered { quarantined, .. } => assert!(quarantined.exists()),
            o => panic!("unexpected {o:?}"),
        }
        assert_eq!(s.load().unwrap().1, LoadOutcome::Loaded);
    }
}
