//! The journal half's storage form: each field stamped with when it last
//! changed, so two devices' concurrent edits merge per field instead of one
//! side clobbering the other.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::decode::decode_tolerant;
use crate::journal::JournalSettings;

/// One field's value and when it was last set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamped {
    pub value: Value,
    pub changed_at: DateTime<Utc>,
}

/// The timestamped storage form of [`JournalSettings`]: a JSON object keyed
/// by field name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StampedFields(BTreeMap<String, Stamped>);

impl StampedFields {
    /// The typed view: [`decode_tolerant`] over the stored values, with the
    /// stamps stripped.
    pub fn view(&self) -> JournalSettings {
        let map: Map<String, Value> = self
            .0
            .iter()
            .map(|(key, stamped)| (key.clone(), stamped.value.clone()))
            .collect();
        decode_tolerant(map)
    }

    /// Stamps every field of `next` whose value differs from the same field
    /// of `self.view()` (the decoded values, defaults included), and no
    /// other. Returns whether anything changed.
    ///
    /// `changed_at` is `now`, unless the field already has a stamp, in which
    /// case it is `max(now, previous changed_at + 1 ms)`: a device whose
    /// clock lags still wins over the value it is replacing.
    pub fn record(&mut self, next: &JournalSettings, now: DateTime<Utc>) -> bool {
        let current = self.view();
        let current_map = as_object(&current);
        let next_map = as_object(next);

        let mut changed = false;
        for (key, next_value) in next_map {
            let current_value = current_map.get(&key).cloned().unwrap_or(Value::Null);
            if current_value == next_value {
                continue;
            }
            let changed_at = match self.0.get(&key) {
                Some(stamped) => now.max(stamped.changed_at + Duration::milliseconds(1)),
                None => now,
            };
            self.0.insert(
                key,
                Stamped {
                    value: next_value,
                    changed_at,
                },
            );
            changed = true;
        }
        changed
    }

    /// Per key over the union of both maps: the later `changed_at` wins,
    /// with a tie broken by the greater canonical JSON text of the value, so
    /// both devices converge on the same result. A key only one side knows
    /// is kept as-is.
    ///
    /// Commutative and associative: the winner per key is the greatest
    /// `(changed_at, canonical text)` pair, which does not depend on which
    /// side is `self` and which is `other`, nor on how a chain of merges is
    /// grouped.
    pub fn merge(&self, other: &Self) -> Self {
        let keys: BTreeSet<&String> = self.0.keys().chain(other.0.keys()).collect();
        let mut result = BTreeMap::new();
        for key in keys {
            let winner = match (self.0.get(key), other.0.get(key)) {
                (Some(a), Some(b)) => {
                    if rank(a) >= rank(b) {
                        a.clone()
                    } else {
                        b.clone()
                    }
                }
                (Some(a), None) => a.clone(),
                (None, Some(b)) => b.clone(),
                (None, None) => unreachable!("key came from one of the two maps"),
            };
            result.insert(key.clone(), winner);
        }
        Self(result)
    }
}

/// The `(changed_at, canonical value text)` pair `merge` orders by.
fn rank(stamped: &Stamped) -> (DateTime<Utc>, String) {
    (
        stamped.changed_at,
        serde_json::to_string(&stamped.value).unwrap_or_default(),
    )
}

fn as_object(settings: &JournalSettings) -> Map<String, Value> {
    match serde_json::to_value(settings) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::journal::TimeFormat;

    fn t(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    #[test]
    fn view_of_empty_fields_is_defaults() {
        assert_eq!(StampedFields::default().view(), JournalSettings::default());
    }

    #[test]
    fn record_on_empty_stamps_only_fields_that_differ_from_defaults() {
        let mut fields = StampedFields::default();
        let next = JournalSettings {
            time_format: TimeFormat::H24,
            ..Default::default()
        };

        let changed = fields.record(&next, t(100));

        assert!(changed);
        assert_eq!(fields.0.len(), 1);
        assert!(fields.0.contains_key("time_format"));
        assert_eq!(fields.view().time_format, TimeFormat::H24);
    }

    #[test]
    fn record_of_no_change_stamps_nothing() {
        let mut fields = StampedFields::default();
        let changed = fields.record(&JournalSettings::default(), t(100));
        assert!(!changed);
        assert!(fields.0.is_empty());
    }

    #[test]
    fn record_on_non_empty_touches_only_the_field_that_changed() {
        let mut fields = StampedFields::default();
        fields.record(
            &JournalSettings {
                time_format: TimeFormat::H24,
                week_starts_on: crate::journal::WeekStart::Monday,
                ..Default::default()
            },
            t(100),
        );
        let week_stamp_before = fields.0.get("week_starts_on").unwrap().changed_at;

        fields.record(
            &JournalSettings {
                time_format: TimeFormat::H12,
                week_starts_on: crate::journal::WeekStart::Monday,
                ..Default::default()
            },
            t(200),
        );

        assert_eq!(
            fields.0.get("week_starts_on").unwrap().changed_at,
            week_stamp_before,
            "a field that did not change must keep its stamp"
        );
        assert_eq!(fields.view().time_format, TimeFormat::H12);
    }

    /// A field this build does not know about (written by a newer version of
    /// the app) must survive both operations untouched: `record` only visits
    /// the keys `JournalSettings` declares, and `merge` keeps a key even when
    /// neither known field set names it.
    #[test]
    fn record_leaves_an_unknown_key_alone() {
        let mut fields = StampedFields::default();
        fields.0.insert(
            "future_field".to_owned(),
            Stamped {
                value: Value::Bool(true),
                changed_at: t(10),
            },
        );

        fields.record(
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
            t(100),
        );

        let stamped = fields.0.get("future_field").unwrap();
        assert_eq!(stamped.value, Value::Bool(true));
        assert_eq!(stamped.changed_at, t(10));
    }

    #[test]
    fn merge_keeps_an_unknown_key_neither_side_declares() {
        let mut a = StampedFields::default();
        a.0.insert(
            "future_field".to_owned(),
            Stamped {
                value: Value::String("x".to_owned()),
                changed_at: t(50),
            },
        );
        let b = StampedFields::default();

        let merged = a.merge(&b);

        assert_eq!(
            merged.0.get("future_field").unwrap().value,
            Value::String("x".to_owned())
        );
    }

    #[test]
    fn record_gives_changed_at_past_a_lagging_clock_and_the_previous_stamp() {
        let mut fields = StampedFields::default();
        fields.record(
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
            t(1000),
        );
        let prev_stamp = fields.0.get("time_format").unwrap().changed_at;

        // The clock now reports an earlier time than the previous stamp.
        fields.record(
            &JournalSettings {
                time_format: TimeFormat::H12,
                ..Default::default()
            },
            t(500),
        );

        let new_stamp = fields.0.get("time_format").unwrap().changed_at;
        assert!(new_stamp > t(500));
        assert!(new_stamp > prev_stamp);
    }

    #[test]
    fn two_empty_stamped_fields_recording_different_changes_merge_to_both() {
        let mut a = StampedFields::default();
        a.record(
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
            t(100),
        );
        let mut b = StampedFields::default();
        b.record(
            &JournalSettings {
                week_starts_on: crate::journal::WeekStart::Monday,
                ..Default::default()
            },
            t(200),
        );

        let merged = a.merge(&b);
        let view = merged.view();
        assert_eq!(view.time_format, TimeFormat::H24);
        assert_eq!(view.week_starts_on, crate::journal::WeekStart::Monday);
    }

    #[test]
    fn merge_is_commutative() {
        let mut a = StampedFields::default();
        a.record(
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
            t(100),
        );
        let mut b = StampedFields::default();
        b.record(
            &JournalSettings {
                time_format: TimeFormat::H12,
                ..Default::default()
            },
            t(100),
        );

        assert_eq!(a.merge(&b), b.merge(&a));
    }

    /// Three sides that all set the *same* key: `a` and `b` tie on
    /// `changed_at` (broken by canonical text), and `c` is strictly later.
    /// Grouping the merges either way must agree, both on the winner and on
    /// which pair's tie-break ran.
    #[test]
    fn merge_is_associative_with_three_sides_disagreeing_on_one_key() {
        let a = StampedFields(BTreeMap::from([(
            "time_format".to_owned(),
            Stamped {
                value: Value::String("h12".to_owned()),
                changed_at: t(100),
            },
        )]));
        let b = StampedFields(BTreeMap::from([(
            "time_format".to_owned(),
            Stamped {
                value: Value::String("h24".to_owned()),
                changed_at: t(100),
            },
        )]));
        let c = StampedFields(BTreeMap::from([(
            "time_format".to_owned(),
            Stamped {
                value: Value::String("locale".to_owned()),
                changed_at: t(200),
            },
        )]));

        assert_eq!(a.merge(&b).merge(&c), a.merge(&b.merge(&c)));
        assert_eq!(
            a.merge(&b).merge(&c).view().time_format,
            TimeFormat::Locale,
            "the strictly later side must win regardless of grouping"
        );
    }

    #[test]
    fn merge_prefers_the_later_stamp() {
        let mut a = StampedFields::default();
        a.record(
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
            t(100),
        );
        let mut b = StampedFields::default();
        b.record(
            &JournalSettings {
                time_format: TimeFormat::H12,
                ..Default::default()
            },
            t(200),
        );

        assert_eq!(a.merge(&b).view().time_format, TimeFormat::H12);
        assert_eq!(b.merge(&a).view().time_format, TimeFormat::H12);
    }

    #[test]
    fn merge_breaks_a_tie_deterministically() {
        let stamp = t(100);
        let a = StampedFields(BTreeMap::from([(
            "time_format".to_owned(),
            Stamped {
                value: Value::String("h12".to_owned()),
                changed_at: stamp,
            },
        )]));
        let b = StampedFields(BTreeMap::from([(
            "time_format".to_owned(),
            Stamped {
                value: Value::String("h24".to_owned()),
                changed_at: stamp,
            },
        )]));

        // "h24" > "h12" lexicographically, so both orders must agree on it.
        assert_eq!(a.merge(&b), b.merge(&a));
        assert_eq!(a.merge(&b).view().time_format, TimeFormat::H24);
    }

    #[test]
    fn merge_keeps_a_key_only_one_side_knows() {
        let mut a = StampedFields::default();
        a.record(
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
            t(100),
        );
        let b = StampedFields::default();

        let merged = a.merge(&b);
        assert_eq!(merged.view().time_format, TimeFormat::H24);
    }
}
