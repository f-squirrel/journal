//! The journal's own settings: encrypted in the vault, synced, merged per
//! field.

use serde::{Deserialize, Serialize};

use crate::decode::Validate;
use crate::error::SettingsError;

/// The entry template's limit `JournalSettings::validate` enforces, in
/// `char`s.
const MAX_TEMPLATE_CHARS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum WeekStart {
    // Today's behaviour: date-fns' `enUS` locale starts the week on Sunday.
    #[default]
    Sunday,
    Monday,
    Saturday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum TimeFormat {
    #[default]
    Locale,
    H12,
    H24,
}

/// The journal's own. Encrypted in the vault, synced, merged per field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
#[serde(default)]
pub struct JournalSettings {
    pub week_starts_on: WeekStart,
    pub time_format: TimeFormat,
    /// Pre-fills a new entry's body. Default "". At most 10_000 chars.
    pub entry_template: String,
}

impl JournalSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.entry_template.chars().count() > MAX_TEMPLATE_CHARS {
            return Err(SettingsError::InvalidValue(format!(
                "the entry template must be at most {MAX_TEMPLATE_CHARS} characters"
            )));
        }
        Ok(())
    }
}

impl Validate for JournalSettings {
    fn validate(&self) -> Result<(), SettingsError> {
        // See the identically named impl on `DeviceSettings`: this resolves
        // to the inherent method above, not to itself.
        self.validate()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_todays_behaviour() {
        let defaults = JournalSettings::default();
        assert_eq!(defaults.week_starts_on, WeekStart::Sunday);
        assert_eq!(defaults.time_format, TimeFormat::Locale);
        assert_eq!(defaults.entry_template, "");
    }

    #[test]
    fn validate_accepts_the_boundary() {
        let settings = JournalSettings {
            entry_template: "a".repeat(MAX_TEMPLATE_CHARS),
            ..Default::default()
        };
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn validate_rejects_over_the_boundary() {
        let settings = JournalSettings {
            entry_template: "a".repeat(MAX_TEMPLATE_CHARS + 1),
            ..Default::default()
        };
        assert!(settings.validate().is_err());
    }
}
