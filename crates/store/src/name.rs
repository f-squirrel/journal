use std::fmt;
use std::ops::Deref;

use crate::StoreError;

/// A name that is safe to address a blob with.
///
/// Validated once, at construction, so a [`BlobStore`](crate::BlobStore)
/// implementation cannot forget to check one: by the time a name reaches a
/// store it is already known not to escape the store's namespace. Every
/// rejected form below is one that addresses somewhere else when pasted into a
/// path — or, for a cloud backend, into a URL.
///
/// Derefs to `str`, so the layers above can ask the questions they care about
/// (`ends_with`, `starts_with`) without unwrapping it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlobName(String);

impl BlobName {
    /// A name that is known good at compile time.
    ///
    /// For literals only — the constants a crate defines for its own blobs.
    /// Panics rather than returning a `Result`, because a literal that fails
    /// this check is a typo in the source, not a condition to handle.
    #[track_caller]
    pub fn literal(name: &'static str) -> Self {
        Self::try_from(name).expect("a blob name literal must be valid")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BlobName {
    type Error = StoreError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let invalid = |reason: &str| Err(StoreError::InvalidName(name.clone(), reason.to_string()));

        if name.is_empty() {
            return invalid("a blob name cannot be empty");
        }
        if name == "." || name == ".." {
            return invalid("a blob name cannot be a relative path component");
        }
        // `\` and `:` are not separators on this platform but are on others, and
        // no name this app generates contains either — rejecting them keeps a
        // blob written on Linux from addressing a different place on Windows.
        if let Some(c) = name.chars().find(|c| matches!(c, '/' | '\\' | ':' | '\0')) {
            return invalid(&format!("a blob name cannot contain {c:?}"));
        }
        Ok(Self(name))
    }
}

impl TryFrom<&str> for BlobName {
    type Error = StoreError;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::try_from(name.to_string())
    }
}

impl Deref for BlobName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for BlobName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BlobName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_app_actually_uses_are_accepted() {
        for name in [
            ".vault-key",
            "5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01.json",
            "a",
            "..leading-dots-are-fine",
        ] {
            let blob =
                BlobName::try_from(name).unwrap_or_else(|e| panic!("rejected {name:?}: {e}"));
            assert_eq!(blob.as_str(), name);
        }
    }

    #[test]
    fn names_that_would_escape_the_store_cannot_be_constructed() {
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "../escape",
            "a\\b",
            "..\\escape",
            "c:evil",
            "nul\0byte",
        ] {
            let Err(err) = BlobName::try_from(name) else {
                panic!("{name:?} was accepted as a blob name");
            };
            assert!(matches!(err, StoreError::InvalidName(..)), "{err:?}");
        }
    }

    #[test]
    fn a_name_answers_questions_about_itself_without_unwrapping() {
        // What the layer above needs in order to tell its own blobs apart from
        // anyone else's in the same store.
        let name = BlobName::literal("5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01.json");
        assert!(name.ends_with(".json"));
        assert!(!BlobName::literal(".vault-key").ends_with(".json"));
    }

    #[test]
    #[should_panic(expected = "a blob name literal must be valid")]
    fn a_bad_literal_fails_loudly() {
        BlobName::literal("../escape");
    }
}
