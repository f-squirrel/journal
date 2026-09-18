use std::fmt;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

/// The SHA-256 of a blob's bytes, as the engine's own record of "what was here".
///
/// Distinct from a remote's content hash: that one is whatever the provider
/// computes and is only ever compared with itself. This one is computed the same
/// way on every device and every provider, which is what lets the engine tell
/// "the local blob changed since the last sync" without asking anyone.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Digest([u8; 32]);

impl Digest {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

/// Serialized as lowercase hex, so the state file stays readable.
impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{self:?}"))
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        if hex.len() != 64 || !hex.is_ascii() {
            return Err(de::Error::custom("a digest is 64 hex characters"));
        }
        let mut bytes = [0u8; 32];
        for (byte, pair) in bytes.iter_mut().zip(hex.as_bytes().chunks(2)) {
            // ASCII was checked above, so every chunk is valid UTF-8.
            let pair = std::str::from_utf8(pair).expect("ascii");
            *byte = u8::from_str_radix(pair, 16).map_err(de::Error::custom)?;
        }
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_round_trips_through_json_as_hex() {
        let digest = Digest::of(b"abc");
        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(
            json,
            "\"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\""
        );
        assert_eq!(serde_json::from_str::<Digest>(&json).unwrap(), digest);
    }

    #[test]
    fn a_malformed_digest_is_refused() {
        for bad in ["\"\"", "\"abc\"", &format!("\"{}\"", "zz".repeat(32))] {
            assert!(serde_json::from_str::<Digest>(bad).is_err(), "{bad}");
        }
    }
}
