//! Tolerant decoding: turning a raw JSON object read from storage into a
//! settings half without ever failing.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::error::SettingsError;

/// Implemented by [`DeviceSettings`](crate::DeviceSettings) and
/// [`JournalSettings`](crate::JournalSettings) so [`decode_tolerant`] and
/// [`apply_patch`](crate::apply_patch) can validate a candidate value without
/// knowing which half they are holding.
///
/// Each half also has an inherent `validate()` with the same signature. The
/// inherent method is what callers that already know the concrete type use
/// (it needs no trait in scope); this trait exists only so the two functions
/// above can be generic over "either half". Rust resolves `value.validate()`
/// to the inherent method whenever the concrete type is known, so the two
/// never conflict; each half's trait impl simply forwards to its own inherent
/// method, which stays the one place the rule lives.
pub trait Validate: Default + Serialize + DeserializeOwned {
    fn validate(&self) -> Result<(), SettingsError>;
}

/// Decodes `map` into `T`, starting from `T::default()` and overlaying each
/// stored field on its own.
///
/// A field is kept only if, once overlaid on the fields already accepted, the
/// whole struct still deserialises *and* validates; otherwise it is dropped
/// and a warning is logged, and the rest of the map is still tried. An
/// unknown key (one `T::default()`'s own serialisation does not contain) is
/// ignored the same way. This is why a corrupt or unknown-format value in
/// storage never stops the app from starting: the caller gets a full `T`,
/// with every field that could not be trusted replaced by its default.
pub fn decode_tolerant<T: Validate>(map: Map<String, Value>) -> T {
    let default = T::default();
    let Ok(Value::Object(default_map)) = serde_json::to_value(&default) else {
        // A settings half always serialises to a JSON object; this is here
        // only so the function has no panicking path.
        return default;
    };

    let mut accepted = default_map.clone();
    for (key, value) in map {
        if !default_map.contains_key(&key) {
            // Not a corrupt value, just a field an older version of the app
            // does not know: this happens on every load once a newer device
            // has written a field this build predates, so it does not
            // deserve a `warn!`.
            log::debug!("ignoring unknown stored setting `{key}`");
            continue;
        }
        let mut candidate = accepted.clone();
        candidate.insert(key.clone(), value);
        match serde_json::from_value::<T>(Value::Object(candidate.clone())) {
            Ok(parsed) if parsed.validate().is_ok() => accepted = candidate,
            _ => {
                log::warn!("ignoring invalid stored setting `{key}`, using its default instead");
            }
        }
    }

    serde_json::from_value(Value::Object(accepted)).unwrap_or(default)
}
