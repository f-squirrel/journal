//! Identifier types: a stable per-entry ID, independent of content, and the
//! content-addressed object name that names a sealed object on disk.

use std::fmt;

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// A stable identifier for a journal entry, chosen once at creation and
/// never changed.
///
/// Objects are content-addressed (crypto spec §4.4): editing an entry seals
/// it under a brand-new object name. Something has to identify "this entry"
/// across edits, and the object name cannot — that's `EntryId`'s job. The
/// manifest ([`crate::manifest::Manifest`]) maps `EntryId` to the entry's
/// current object name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(Uuid);

impl EntryId {
    /// Mints a new, randomly chosen entry ID.
    #[must_use]
    pub fn new_random() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// The length in bytes of an object name tag (crypto spec §4.4): 160 bits.
pub const OBJECT_NAME_LEN: usize = 20;

/// The content-addressed name of a sealed object.
///
/// This is the raw 20-byte keyed tag (crypto spec §4.4) — `HMAC(K_name, kind
/// || SHA-256(content))`, truncated — not the 32-character Crockford Base32
/// encoding used for on-disk paths. Deriving the tag from `K_name` and
/// encoding it for a path is `journal-crypto`'s job (spec §4.4); this crate
/// has no crypto dependency, and only carries the value so [`crate::entry::Entry`]
/// and [`crate::attachment::Attachment`] can point at other objects.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectName([u8; OBJECT_NAME_LEN]);

impl ObjectName {
    #[must_use]
    pub fn from_bytes(bytes: [u8; OBJECT_NAME_LEN]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; OBJECT_NAME_LEN] {
        &self.0
    }
}

impl fmt::Debug for ObjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectName({})", hex::encode(self.0))
    }
}

impl fmt::Display for ObjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl Serialize for ObjectName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for ObjectName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        let bytes = hex::decode(&encoded).map_err(D::Error::custom)?;
        let array: [u8; OBJECT_NAME_LEN] =
            bytes.try_into().map_err(|_| D::Error::custom("object name must be 20 bytes"))?;
        Ok(Self(array))
    }
}

#[cfg(test)]
mod tests {
    use super::{EntryId, ObjectName, OBJECT_NAME_LEN};

    #[test]
    fn entry_id_round_trips_through_json() -> Result<(), Box<dyn std::error::Error>> {
        let id = EntryId::new_random();
        let json = serde_json::to_string(&id)?;
        let parsed: EntryId = serde_json::from_str(&json)?;
        assert_eq!(id, parsed);
        Ok(())
    }

    #[test]
    fn object_name_round_trips_through_json() -> Result<(), Box<dyn std::error::Error>> {
        let name = ObjectName::from_bytes([7u8; OBJECT_NAME_LEN]);
        let json = serde_json::to_string(&name)?;
        assert_eq!(json, "\"0707070707070707070707070707070707070707\"");
        let parsed: ObjectName = serde_json::from_str(&json)?;
        assert_eq!(name, parsed);
        Ok(())
    }

    #[test]
    fn object_name_rejects_wrong_length() {
        let result: Result<ObjectName, _> = serde_json::from_str("\"aabb\"");
        assert!(result.is_err(), "a 2-byte hex string must not parse as a 20-byte object name");
    }

    #[test]
    fn object_name_rejects_non_hex() {
        let result: Result<ObjectName, _> =
            serde_json::from_str("\"not-hex-not-hex-not-hex-not-hex!!!!\"");
        assert!(result.is_err());
    }
}
