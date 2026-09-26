//! This device's own settings: plaintext, never synced, readable while the
//! journal is locked.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::decode::{decode_tolerant, Validate};
use crate::error::SettingsError;

/// The at most 64 shortcut ids or chords `DeviceSettings::validate` allows, in
/// bytes.
const MAX_SHORTCUT_LEN: usize = 64;
/// The at most 64 shortcut overrides `DeviceSettings::validate` allows.
const MAX_SHORTCUTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    #[default]
    System,
    Light,
    Dark,
}

/// This device only. Plaintext, never synced, readable while locked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
#[serde(default)]
pub struct DeviceSettings {
    pub color_mode: ColorMode,
    /// `None` = never lock on idle. Default `Some(5)`. Valid: 1..=240.
    pub idle_lock_minutes: Option<u16>,
    /// `None` = no periodic pass: sync only at startup, after saves, and on
    /// "Sync now". Default `Some(1)` (today's 60 s). Valid: 1..=1440.
    pub sync_interval_minutes: Option<u16>,
    /// Action id → chord, or `None` for an explicit unbind. Opaque to Rust
    /// beyond size limits: the action registry and its chord rules live in
    /// the frontend (ADR 0015), so the frontend still sanitises what it
    /// reads here.
    pub shortcuts: BTreeMap<String, Option<String>>,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            color_mode: ColorMode::default(),
            idle_lock_minutes: Some(5),
            sync_interval_minutes: Some(1),
            shortcuts: BTreeMap::new(),
        }
    }
}

impl DeviceSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if let Some(minutes) = self.idle_lock_minutes {
            if !(1..=240).contains(&minutes) {
                return Err(SettingsError::InvalidValue(
                    "the idle lock must be between 1 and 240 minutes".to_owned(),
                ));
            }
        }
        if let Some(minutes) = self.sync_interval_minutes {
            if !(1..=1440).contains(&minutes) {
                return Err(SettingsError::InvalidValue(
                    "the sync check interval must be between 1 and 1440 minutes".to_owned(),
                ));
            }
        }
        if self.shortcuts.len() > MAX_SHORTCUTS {
            return Err(SettingsError::InvalidValue(format!(
                "at most {MAX_SHORTCUTS} keyboard shortcuts are allowed"
            )));
        }
        for (id, chord) in &self.shortcuts {
            if id.len() > MAX_SHORTCUT_LEN {
                return Err(SettingsError::InvalidValue(format!(
                    "shortcut id `{id}` is too long"
                )));
            }
            if let Some(chord) = chord {
                if chord.len() > MAX_SHORTCUT_LEN {
                    return Err(SettingsError::InvalidValue(format!(
                        "the chord for shortcut `{id}` is too long"
                    )));
                }
            }
        }
        Ok(())
    }
}

impl Validate for DeviceSettings {
    fn validate(&self) -> Result<(), SettingsError> {
        // Resolves to the inherent method above, not this one: Rust prefers
        // an inherent method over a trait method of the same name on the
        // same receiver type, so this is not infinite recursion.
        self.validate()
    }
}

/// Reads and writes [`DeviceSettings`] as pretty-printed plaintext JSON at a
/// fixed path, tolerant of a missing or corrupt file.
pub struct DeviceSettingsFile {
    path: PathBuf,
}

impl DeviceSettingsFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// A missing file gives defaults. An unreadable or unparseable file also
    /// gives defaults, plus a `warn!`; the file is left untouched until the
    /// next [`save`](Self::save).
    pub fn load(&self) -> DeviceSettings {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return DeviceSettings::default(),
            Err(e) => {
                log::warn!("failed to read device settings file: {e}");
                return DeviceSettings::default();
            }
        };
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(e) => {
                log::warn!("device settings file is not valid JSON: {e}");
                return DeviceSettings::default();
            }
        };
        match value {
            Value::Object(map) => decode_tolerant(map),
            _ => {
                log::warn!("device settings file did not hold a JSON object");
                DeviceSettings::default()
            }
        }
    }

    /// Temp file plus rename, as `FsStore::put` does: write, `sync_all`, then
    /// rename into place, with a best-effort directory sync afterwards so the
    /// new name survives a crash. Creates the parent directory if needed.
    ///
    /// The scratch file's name is fixed (`<path>.json.tmp`), so two calls
    /// racing on the same `DeviceSettingsFile` would clobber each other's
    /// temp file: callers must serialise their saves (`src-tauri`'s
    /// `DeviceSettingsState` does this by holding its lock across the write).
    pub fn save(&self, settings: &DeviceSettings) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
        let temp = self.path.with_extension("json.tmp");
        if let Err(e) = self
            .write_temp(&temp, json.as_bytes())
            .and_then(|()| std::fs::rename(&temp, &self.path))
        {
            std::fs::remove_file(&temp).ok();
            return Err(e);
        }
        self.sync_dir();
        Ok(())
    }

    /// Writes `bytes` to `temp` and syncs them, so a name given to the file
    /// afterwards is one a reader can trust.
    fn write_temp(&self, temp: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = std::fs::File::create(temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    /// Syncs the parent directory, best-effort: not every platform lets a
    /// directory be opened for sync.
    fn sync_dir(&self) {
        if let Some(parent) = self.path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_d5_and_d6() {
        let defaults = DeviceSettings::default();
        assert_eq!(defaults.color_mode, ColorMode::System);
        assert_eq!(defaults.idle_lock_minutes, Some(5));
        assert_eq!(defaults.sync_interval_minutes, Some(1));
        assert!(defaults.shortcuts.is_empty());
    }

    #[test]
    fn validate_accepts_boundaries() {
        let mut settings = DeviceSettings {
            idle_lock_minutes: Some(1),
            sync_interval_minutes: Some(1),
            ..Default::default()
        };
        assert!(settings.validate().is_ok());
        settings.idle_lock_minutes = Some(240);
        settings.sync_interval_minutes = Some(1440);
        assert!(settings.validate().is_ok());
        settings.idle_lock_minutes = None;
        settings.sync_interval_minutes = None;
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn validate_rejects_out_of_range() {
        let mut settings = DeviceSettings {
            idle_lock_minutes: Some(0),
            ..Default::default()
        };
        assert!(settings.validate().is_err());
        settings.idle_lock_minutes = Some(241);
        assert!(settings.validate().is_err());

        let mut settings = DeviceSettings {
            sync_interval_minutes: Some(0),
            ..Default::default()
        };
        assert!(settings.validate().is_err());
        settings.sync_interval_minutes = Some(1441);
        assert!(settings.validate().is_err());
    }

    #[test]
    fn validate_rejects_too_many_or_too_long_shortcuts() {
        let mut settings = DeviceSettings::default();
        for i in 0..65 {
            settings
                .shortcuts
                .insert(format!("action.{i}"), Some("$mod+A".to_owned()));
        }
        assert!(settings.validate().is_err());

        let mut settings = DeviceSettings::default();
        settings
            .shortcuts
            .insert("a".repeat(65), Some("$mod+A".to_owned()));
        assert!(settings.validate().is_err());

        let mut settings = DeviceSettings::default();
        settings
            .shortcuts
            .insert("a".to_owned(), Some("x".repeat(65)));
        assert!(settings.validate().is_err());

        let mut settings = DeviceSettings::default();
        settings.shortcuts.insert("a".to_owned(), None);
        assert!(settings.validate().is_ok());
    }

    fn temp_path() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "journal-settings-test-{}-{n}.json",
            std::process::id()
        ))
    }

    #[test]
    fn load_of_missing_file_is_defaults() {
        let file = DeviceSettingsFile::new(temp_path());
        assert_eq!(file.load(), DeviceSettings::default());
    }

    #[test]
    fn load_of_corrupt_file_is_defaults_and_leaves_the_file_untouched() {
        let path = temp_path();
        std::fs::write(&path, b"not json").unwrap();
        let file = DeviceSettingsFile::new(&path);
        assert_eq!(file.load(), DeviceSettings::default());
        // The file is not overwritten until the next `save` (see `load`'s doc
        // comment).
        assert_eq!(std::fs::read(&path).unwrap(), b"not json");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_of_partly_valid_file_keeps_the_valid_fields() {
        let path = temp_path();
        std::fs::write(&path, br#"{"color_mode":"dark","idle_lock_minutes":9999}"#).unwrap();
        let file = DeviceSettingsFile::new(&path);

        let loaded = file.load();

        assert_eq!(loaded.color_mode, ColorMode::Dark);
        assert_eq!(loaded.idle_lock_minutes, Some(5));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = temp_path();
        let file = DeviceSettingsFile::new(&path);
        let mut settings = DeviceSettings {
            color_mode: ColorMode::Light,
            idle_lock_minutes: None,
            ..Default::default()
        };
        settings
            .shortcuts
            .insert("view.settings".to_owned(), Some("$mod+Comma".to_owned()));

        file.save(&settings).unwrap();
        let loaded = file.load();

        assert_eq!(loaded, settings);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn save_creates_the_parent_directory() {
        let dir =
            std::env::temp_dir().join(format!("journal-settings-test-dir-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("device-settings.json");
        let file = DeviceSettingsFile::new(&path);

        file.save(&DeviceSettings::default()).unwrap();

        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
