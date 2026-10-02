//! Settings, defined in Rust and shared between the frontend, the device's
//! plaintext config file, and the journal's encrypted, synced blob.
//!
//! The frontend never sees the split: it gets one flat [`Settings`], built by
//! flattening [`DeviceSettings`] (this device only, plaintext, never synced)
//! and [`JournalSettings`] (the journal's own, encrypted in the vault, synced
//! and merged per field). Which half a setting belongs to is decided by which
//! struct declares it, so the compiler enforces that every setting has
//! exactly one home.
//!
//! This crate holds the types, their validation, tolerant decoding from
//! storage, strict patch application from the UI, the journal half's
//! timestamped storage form and its merge, and the device half's file store.
//! `journal-vault` stores and merges the journal half; `journal-api`
//! re-exports the wire types; `src-tauri` owns the device file store and
//! applies patches. See `.claude/plans/device-and-journal-config.md`.

#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

mod decode;
mod device;
mod error;
mod journal;
mod patch;
mod stamped;

pub use decode::{decode_tolerant, Validate};
pub use device::{
    ColorMode, ColorScheme, DeviceSettings, DeviceSettingsFile, DropboxSettings, SyncProviders,
};
pub use error::SettingsError;
pub use journal::{JournalSettings, TimeFormat, WeekStart};
pub use patch::{apply_patch, Patched};
pub use stamped::{Stamped, StampedFields};

use serde::{Deserialize, Serialize};

/// What the frontend sees: both halves as one flat object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
pub struct Settings {
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub device: DeviceSettings,
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub journal: JournalSettings,
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn defaults_round_trip() {
        let settings = Settings::default();
        let value = serde_json::to_value(&settings).unwrap();
        let back: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(settings, back);
    }

    #[test]
    fn settings_serialises_flat() {
        let settings = Settings::default();
        let value = serde_json::to_value(&settings).unwrap();
        let Value::Object(map) = &value else {
            panic!("expected an object");
        };

        for key in [
            "color_mode",
            "color_scheme",
            "idle_lock_minutes",
            "sync_interval_minutes",
            "shortcuts",
            "sync_providers",
            "week_starts_on",
            "time_format",
            "entry_template",
        ] {
            assert!(map.contains_key(key), "missing top-level key `{key}`");
        }
        // Flat: no `device` or `journal` sub-object.
        assert!(map.get("device").is_none());
        assert!(map.get("journal").is_none());
    }

    #[test]
    fn the_two_halves_key_sets_are_disjoint() {
        let device_value = serde_json::to_value(DeviceSettings::default()).unwrap();
        let journal_value = serde_json::to_value(JournalSettings::default()).unwrap();
        let (Value::Object(device_map), Value::Object(journal_map)) = (device_value, journal_value)
        else {
            panic!("expected objects");
        };

        for key in device_map.keys() {
            assert!(
                !journal_map.contains_key(key),
                "key `{key}` is in both halves"
            );
        }
    }

    #[test]
    fn a_missing_field_takes_its_default() {
        let value = json!({"color_mode": "dark"});
        let Value::Object(map) = value else {
            unreachable!()
        };
        let device: DeviceSettings = decode_tolerant(map);
        assert_eq!(device.color_mode, ColorMode::Dark);
        assert_eq!(device.color_scheme, ColorScheme::Parchment);
        assert_eq!(device.idle_lock_minutes, Some(5));
        assert_eq!(device.sync_interval_minutes, Some(1));
    }

    #[test]
    fn one_invalid_field_falls_back_alone_others_survive() {
        // Out of range, wrong type, and an unknown enum value, mixed with one
        // valid field.
        let value = json!({
            "color_mode": "not_a_color",
            "color_scheme": "neon",
            "idle_lock_minutes": 500,
            "sync_interval_minutes": "soon",
            "week_starts_on": "monday",
        });
        let Value::Object(device_map) = value.clone() else {
            unreachable!()
        };
        let device: DeviceSettings = decode_tolerant(device_map);
        assert_eq!(device.color_mode, ColorMode::System, "unknown enum value");
        assert_eq!(
            device.color_scheme,
            ColorScheme::Parchment,
            "unknown scheme id"
        );
        assert_eq!(device.idle_lock_minutes, Some(5), "out of range");
        assert_eq!(device.sync_interval_minutes, Some(1), "wrong type");

        let Value::Object(journal_map) = value else {
            unreachable!()
        };
        let journal: JournalSettings = decode_tolerant(journal_map);
        assert_eq!(
            journal.week_starts_on,
            WeekStart::Monday,
            "the valid field must survive"
        );
    }

    #[test]
    fn unknown_stored_keys_are_ignored() {
        let value = json!({"this_field_does_not_exist": true, "color_mode": "light"});
        let Value::Object(map) = value else {
            unreachable!()
        };
        let device: DeviceSettings = decode_tolerant(map);
        assert_eq!(device.color_mode, ColorMode::Light);
    }
}
