//! Strict patch application: turning a `Partial<Settings>`-shaped JSON map
//! from the UI into validated, half-scoped changes.

use std::collections::HashSet;

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};

use crate::decode::Validate;
use crate::device::DeviceSettings;
use crate::error::SettingsError;
use crate::journal::JournalSettings;
use crate::Settings;

/// The result of [`apply_patch`]: which half(s) actually changed.
#[derive(Debug)]
pub struct Patched {
    /// `Some` when the device half changed.
    pub device: Option<DeviceSettings>,
    /// `Some` when the journal half changed.
    pub journal: Option<JournalSettings>,
    /// True when the patch names any journal key, even one whose value ends
    /// up unchanged. Used for the locked check, which must refuse by key, not
    /// by whether anything would actually change (see `src-tauri`'s
    /// `update_settings`).
    pub names_journal: bool,
}

/// Applies `patch` (a JSON object, as the UI's `Partial<Settings>` becomes
/// once it crosses `invoke`) to `current`.
///
/// Each key goes to the half whose default serialisation contains it. A key
/// neither half has is refused with [`SettingsError::UnknownField`]; a value
/// that does not deserialise as its field's type, or a changed half that
/// fails `validate`, is refused with [`SettingsError::InvalidValue`]. Nothing
/// is written when refused.
pub fn apply_patch(
    current: &Settings,
    patch: Map<String, Value>,
) -> Result<Patched, SettingsError> {
    let device_keys = default_keys::<DeviceSettings>();
    let journal_keys = default_keys::<JournalSettings>();

    let mut device_map = as_object(&current.device);
    let mut journal_map = as_object(&current.journal);
    let mut touched_device = false;
    let mut names_journal = false;

    for (key, value) in patch {
        if device_keys.contains(&key) {
            touched_device = true;
            try_insert::<DeviceSettings>(&mut device_map, &key, value)?;
        } else if journal_keys.contains(&key) {
            names_journal = true;
            try_insert::<JournalSettings>(&mut journal_map, &key, value)?;
        } else {
            return Err(SettingsError::UnknownField(key));
        }
    }

    let device = touched_device
        .then(|| apply_half::<DeviceSettings>(device_map, &current.device))
        .transpose()?
        .flatten();
    let journal = names_journal
        .then(|| apply_half::<JournalSettings>(journal_map, &current.journal))
        .transpose()?
        .flatten();

    Ok(Patched {
        device,
        journal,
        names_journal,
    })
}

/// Inserts `value` for `key` into `map`, refusing it if the resulting object
/// would not deserialise as `T`.
///
/// The error names the key, not the value: echoing the value back would put
/// whatever the user typed into a shortcut chord or a template into a log or
/// an error dialog verbatim.
fn try_insert<T: DeserializeOwned>(
    map: &mut Map<String, Value>,
    key: &str,
    value: Value,
) -> Result<(), SettingsError> {
    let mut candidate = map.clone();
    candidate.insert(key.to_owned(), value);
    if serde_json::from_value::<T>(Value::Object(candidate.clone())).is_err() {
        return Err(SettingsError::InvalidValue(format!(
            "invalid value for `{key}`"
        )));
    }
    *map = candidate;
    Ok(())
}

/// Parses and validates one half's overlaid map, returning `None` when the
/// result equals `current` (nothing to write).
///
/// The type-checking [`try_insert`] already did for each key means this
/// parse should never fail; the fallback error stays value-free regardless.
fn apply_half<T>(map: Map<String, Value>, current: &T) -> Result<Option<T>, SettingsError>
where
    T: DeserializeOwned + PartialEq + Validate,
{
    let parsed: T = serde_json::from_value(Value::Object(map))
        .map_err(|_| SettingsError::InvalidValue("invalid settings value".to_owned()))?;
    parsed.validate()?;
    Ok((parsed != *current).then_some(parsed))
}

fn default_keys<T: Default + Serialize>() -> HashSet<String> {
    match serde_json::to_value(T::default()) {
        Ok(Value::Object(map)) => map.keys().cloned().collect(),
        _ => HashSet::new(),
    }
}

fn as_object<T: Serialize>(value: &T) -> Map<String, Value> {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::device::ColorMode;
    use crate::journal::TimeFormat;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(m) => m,
            _ => panic!("expected an object"),
        }
    }

    #[test]
    fn routes_each_key_to_its_half() {
        let current = Settings::default();
        let patch = map(json!({"color_mode": "dark", "time_format": "h24"}));

        let patched = apply_patch(&current, patch).unwrap();

        assert_eq!(
            patched.device.as_ref().map(|d| d.color_mode),
            Some(ColorMode::Dark)
        );
        assert_eq!(
            patched.journal.as_ref().map(|j| j.time_format),
            Some(TimeFormat::H24)
        );
    }

    #[test]
    fn an_untouched_half_is_none() {
        let current = Settings::default();
        let patch = map(json!({"color_mode": "dark"}));

        let patched = apply_patch(&current, patch).unwrap();

        assert!(patched.device.is_some());
        assert!(patched.journal.is_none());
        assert!(!patched.names_journal);
    }

    #[test]
    fn names_journal_is_set_for_an_unchanged_journal_value() {
        let current = Settings::default();
        // `time_format` already defaults to "locale".
        let patch = map(json!({"time_format": "locale"}));

        let patched = apply_patch(&current, patch).unwrap();

        assert!(patched.names_journal);
        assert!(patched.journal.is_none(), "value did not change");
    }

    #[test]
    fn names_journal_is_false_for_a_device_only_patch() {
        let current = Settings::default();
        let patch = map(json!({"color_mode": "dark"}));

        let patched = apply_patch(&current, patch).unwrap();

        assert!(!patched.names_journal);
    }

    #[test]
    fn null_sets_idle_lock_to_never_and_a_missing_key_leaves_it_alone() {
        let mut current = Settings::default();
        current.device.idle_lock_minutes = Some(15);

        let patch = map(json!({"idle_lock_minutes": null}));
        let patched = apply_patch(&current, patch).unwrap();
        assert_eq!(patched.device.as_ref().unwrap().idle_lock_minutes, None);

        let patch = map(json!({"color_mode": "dark"}));
        let patched = apply_patch(&current, patch).unwrap();
        assert_eq!(patched.device.as_ref().unwrap().idle_lock_minutes, Some(15));
    }

    #[test]
    fn refuses_an_unknown_key() {
        let current = Settings::default();
        let patch = map(json!({"nonexistent_field": 1}));

        let err = apply_patch(&current, patch).unwrap_err();
        assert!(matches!(err, SettingsError::UnknownField(k) if k == "nonexistent_field"));
    }

    #[test]
    fn refuses_a_wrong_type() {
        let current = Settings::default();
        let patch = map(json!({"idle_lock_minutes": "soon"}));

        let err = apply_patch(&current, patch).unwrap_err();
        assert!(matches!(err, SettingsError::InvalidValue(_)));
    }

    /// The message must say which key was wrong, and must not repeat the
    /// value the user (or a bug) sent — that value could be a shortcut chord
    /// or a chunk of the entry template, and does not belong in an error
    /// string or a log line.
    #[test]
    fn a_wrong_type_error_names_the_key_and_not_the_value() {
        let current = Settings::default();
        let patch = map(json!({"idle_lock_minutes": "a very particular secret value"}));

        let err = apply_patch(&current, patch).unwrap_err();

        let SettingsError::InvalidValue(message) = err else {
            panic!("expected InvalidValue, got {err:?}");
        };
        assert!(
            message.contains("idle_lock_minutes"),
            "message did not name the key: {message}"
        );
        assert!(
            !message.contains("a very particular secret value"),
            "message echoed the value: {message}"
        );
    }

    #[test]
    fn refuses_an_out_of_range_value() {
        let current = Settings::default();
        let patch = map(json!({"idle_lock_minutes": 0}));

        let err = apply_patch(&current, patch).unwrap_err();
        assert!(matches!(err, SettingsError::InvalidValue(_)));
    }

    #[test]
    fn refuses_nothing_written_on_error() {
        let current = Settings::default();
        let patch = map(json!({"color_mode": "dark", "nonexistent_field": 1}));

        assert!(apply_patch(&current, patch).is_err());
    }
}
