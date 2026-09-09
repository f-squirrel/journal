//! The local filesystem as a [`BlobStore`](journal_store::BlobStore).
//!
//! Separate from the trait it implements (`journal-store`) so that the layers
//! above can depend on the abstraction alone: only the app's wiring names a
//! concrete store, and a cloud backend arrives as a sibling crate rather than
//! as a variant inside this one (ADR 0009).

use std::fs;
use std::path::{Path, PathBuf};

use journal_store::{BlobName, BlobStore, StoreError};

/// A [`BlobStore`] backed by one file per blob in a single directory.
///
/// The directory is created on first write rather than at construction, so
/// naming a store costs nothing and a store that has never been written to
/// reads as empty instead of failing.
#[derive(Debug, Clone)]
pub struct FsStore {
    dir: PathBuf,
}

impl FsStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory this store keeps its blobs in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The path a blob lives at.
    ///
    /// Infallible: a [`BlobName`] cannot hold a separator or a `..`, so joining
    /// one onto the store's directory always stays inside it.
    fn path(&self, name: &BlobName) -> PathBuf {
        self.dir.join(name.as_str())
    }
}

impl BlobStore for FsStore {
    fn get(&self, name: &BlobName) -> Result<Option<Vec<u8>>, StoreError> {
        match fs::read(self.path(name)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn put(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
        fs::create_dir_all(&self.dir)?;
        fs::write(self.path(name), bytes)?;
        Ok(())
    }

    fn delete(&self, name: &BlobName) -> Result<(), StoreError> {
        match fs::remove_file(self.path(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self) -> Result<Vec<BlobName>, StoreError> {
        let dir = match fs::read_dir(&self.dir) {
            Ok(dir) => dir,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut names = Vec::new();
        for file in dir {
            let file = file?;
            // `is_file` is load-bearing: a *directory* named like a blob is
            // reachable, and a caller that counted one would decide the store
            // holds data it cannot read.
            if !file.path().is_file() {
                continue;
            }
            // A filename this store could not have written — something not
            // UTF-8, or holding a character no `BlobName` allows — is not ours
            // to report. Skipped rather than raised: the directory belongs to
            // the app, but nothing stops a user or a sync client dropping a
            // file into it.
            match file.file_name().into_string().map(BlobName::try_from) {
                Ok(Ok(name)) => names.push(name),
                _ => continue,
            }
        }
        Ok(names)
    }

    fn contains(&self, name: &BlobName) -> Result<bool, StoreError> {
        Ok(self.path(name).is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A directory name no other test in this process will pick.
    fn temp_store() -> FsStore {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        FsStore::new(
            std::env::temp_dir().join(format!("journal-fs-store-test-{}-{n}", std::process::id())),
        )
    }

    fn name(name: &'static str) -> BlobName {
        BlobName::literal(name)
    }

    #[test]
    fn put_then_get_returns_the_bytes() {
        let store = temp_store();
        store.put(&name("a.json"), b"hello").unwrap();
        assert_eq!(store.get(&name("a.json")).unwrap().unwrap(), b"hello");
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn put_replaces_an_existing_blob() {
        let store = temp_store();
        store.put(&name("a.json"), b"first").unwrap();
        store.put(&name("a.json"), b"second").unwrap();
        assert_eq!(store.get(&name("a.json")).unwrap().unwrap(), b"second");
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn a_missing_blob_reads_as_none_not_an_error() {
        let store = temp_store();
        assert!(store.get(&name("nope.json")).unwrap().is_none());
        assert!(!store.contains(&name("nope.json")).unwrap());
    }

    #[test]
    fn delete_removes_the_blob_and_is_idempotent() {
        let store = temp_store();
        store.put(&name("a.json"), b"hello").unwrap();
        store.delete(&name("a.json")).unwrap();
        assert!(store.get(&name("a.json")).unwrap().is_none());
        // Deleting again must not fail: an entry the user deleted twice, or a
        // blob a sync engine already removed, is not an error condition.
        store.delete(&name("a.json")).unwrap();
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn list_names_every_blob_written() {
        let store = temp_store();
        store.put(&name("a.json"), b"1").unwrap();
        store.put(&name("b.json"), b"2").unwrap();
        store.put(&name(".vault-key"), b"3").unwrap();

        let mut names = store.list().unwrap();
        names.sort();
        assert_eq!(names, [name(".vault-key"), name("a.json"), name("b.json")]);
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn list_skips_directories_named_like_blobs() {
        let store = temp_store();
        fs::create_dir_all(store.dir().join("a.json")).unwrap();
        store.put(&name("b.json"), b"2").unwrap();

        assert_eq!(store.list().unwrap(), [name("b.json")]);
        assert!(!store.contains(&name("a.json")).unwrap());
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn list_skips_files_this_store_could_not_have_written() {
        // `:` is a legal filename character here and an illegal `BlobName` one,
        // so this is a file in the directory that no blob name addresses. It
        // must not be reported as a blob, because a caller would then ask for
        // it by a name it cannot construct.
        let store = temp_store();
        store.put(&name("a.json"), b"1").unwrap();
        fs::write(store.dir().join("dropped:by-someone-else"), b"?").unwrap();

        assert_eq!(store.list().unwrap(), [name("a.json")]);
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn a_store_that_was_never_written_lists_empty() {
        // First run: the directory does not exist yet. "Nothing here" is the
        // right answer, and the caller decides whether that means first launch.
        let store = temp_store();
        assert!(store.list().unwrap().is_empty());
        assert!(!store.dir().exists(), "reading created the directory");
    }

    #[test]
    fn a_blob_always_lands_inside_the_store() {
        // The traversal case this store used to check for itself. It is now
        // unrepresentable — `BlobName` refuses `../escape` — so what is left to
        // verify is that a valid name resolves where it should.
        let store = temp_store();
        store.put(&name("real.json"), b"1").unwrap();

        assert!(store.dir().join("real.json").is_file());
        assert!(BlobName::try_from("../escaped.json").is_err());
        fs::remove_dir_all(store.dir()).ok();
    }
}
