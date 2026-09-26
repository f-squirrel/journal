use thiserror::Error;

/// Why a patch or a stored value was refused.
///
/// Both variants carry a user-readable message: the UI only ever offers
/// valid values, so surfacing either of these means a bug, and the message
/// is what shows up while it gets fixed.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SettingsError {
    /// A patch key that neither [`DeviceSettings`](crate::DeviceSettings) nor
    /// [`JournalSettings`](crate::JournalSettings) declares.
    #[error("unknown setting: `{0}`")]
    UnknownField(String),
    /// A patch value that does not deserialise as its field's type, or a
    /// value that deserialises but fails [`validate`](crate::DeviceSettings::validate).
    #[error("{0}")]
    InvalidValue(String),
}
