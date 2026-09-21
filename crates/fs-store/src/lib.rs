//! The local filesystem as a [`BlobStore`](journal_store::BlobStore).
//!
//! Separate from the trait it implements (`journal-store`) so that the layers
//! above can depend on the abstraction alone: only the app's wiring names a
//! concrete store, and a cloud backend arrives as a sibling crate rather than
//! as a variant inside this one (ADR 0009).

#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use journal_store::{BlobName, BlobStore, StoreError};

/// What a half-written blob is called until it is renamed into place.
///
/// Files ending in this are the store's own scratch, not blobs: [`FsStore::list`]
/// skips them, so a crash between the write and the rename leaves litter rather
/// than an entry the layers above try to decrypt.
const TEMP_EXT: &str = ".tmp";

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

    /// A scratch path in the same directory, used once and never reused.
    ///
    /// Same directory because [`put`](FsStore::put) finishes with a rename, and
    /// rename is only atomic within one filesystem. Unique per call because two
    /// threads saving the same entry would otherwise write the same temp file:
    /// interleaving *there* tears the blob just as a shared `fs::write` would,
    /// which is the whole defect this is here to fix.
    fn temp_path(&self, name: &BlobName) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        self.dir.join(format!(
            "{}.{}-{n}{TEMP_EXT}",
            name.as_str(),
            std::process::id()
        ))
    }

    /// Writes the bytes to `temp` and syncs them, so a name given to the file
    /// afterwards is one a reader can trust.
    fn write_temp(&self, temp: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let mut file = fs::File::create(temp)?;
        file.write_all(bytes)?;
        // The bytes, before the caller gives them a name a reader will trust.
        file.sync_all()
    }

    /// Syncs the directory, so a new name in it survives a crash. Best-effort:
    /// not every platform lets a directory be opened for sync.
    fn sync_dir(&self) {
        if let Ok(dir) = fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
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

    /// Writes the bytes to a temp file, syncs it, and renames it into place.
    ///
    /// *Atomicity* is the rename. A bare `fs::write` truncates before it
    /// writes, so a reader — or a second writer — that arrives mid-call sees a
    /// torn blob. Commands have run concurrently since they moved off the main
    /// thread (P1-1), and the truncation window is an entry lost rather than an
    /// entry stale. Rename is atomic within a directory on every platform the
    /// app targets, so a reader sees either the old blob or the new one.
    ///
    /// *Durability* is the two syncs, and the rename alone does not give it.
    /// ext4's `auto_da_alloc` forces writeback when a rename replaces an
    /// existing file, which covers edits but not the first save of a new
    /// name; APFS needs `F_FULLFSYNC` for ordering and a plain rename does not
    /// imply it. Without the file sync, a crash inside the writeback window
    /// leaves a present but zero-length blob — one that fails to decrypt
    /// forever, so `Vault::list_entries` skips it with a `warn!` and the entry
    /// is gone from the UI with nothing to repair it. One `fsync` per save is
    /// affordable here: a save is user-initiated and infrequent. Do not batch
    /// it.
    fn put(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
        fs::create_dir_all(&self.dir)?;
        let temp = self.temp_path(name);

        // Everything that can fail once `temp` exists is in this one chain, so
        // the cleanup below covers all of it rather than the rename alone.
        if let Err(e) = self
            .write_temp(&temp, bytes)
            .and_then(|()| fs::rename(&temp, self.path(name)))
        {
            // The blob is untouched, but the scratch file would otherwise stay
            // in the store's directory for good. `File::create` on a missing
            // parent fails before `temp` exists, so this is a no-op there
            // rather than wrong.
            fs::remove_file(&temp).ok();
            return Err(e.into());
        }

        // The rename itself, so the new name survives a crash.
        self.sync_dir();
        Ok(())
    }

    /// Creates the blob, or fails if the name is taken.
    ///
    /// A link rather than a rename: `rename` replaces silently, which is the
    /// whole property this method exists to deny. `hard_link` fails with
    /// `AlreadyExists` if the name is there, atomically, on both POSIX and
    /// NTFS — and the content is written and synced first, so this keeps the
    /// durability [`put`](FsStore::put)'s doc comment argues for.
    fn put_new(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
        fs::create_dir_all(&self.dir)?;
        let temp = self.temp_path(name);
        let link = self
            .write_temp(&temp, bytes)
            .and_then(|()| fs::hard_link(&temp, self.path(name)));
        // The scratch file goes either way: on success the blob has its real
        // name, on failure nothing was created.
        fs::remove_file(&temp).ok();
        match link {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(StoreError::AlreadyExists(name.as_str().to_owned()));
            }
            Err(e) => return Err(e.into()),
        }
        self.sync_dir();
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
            // Scratch from a `put` that was interrupted between its write and
            // its rename. It is not a blob, and reporting it as one would hand
            // the layer above a half-written entry to decrypt.
            if file
                .file_name()
                .as_encoded_bytes()
                .ends_with(TEMP_EXT.as_bytes())
            {
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
#[cfg_attr(coverage_nightly, coverage(off))]
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

    /// R1-1 guard (green before and after): `put` grew a `File::create` and two
    /// syncs, and the directory it writes into is still created on first write.
    /// Dropping the `create_dir_all` would fail only on a first-ever save —
    /// the one case no other test here covers.
    #[test]
    fn put_creates_the_store_directory_on_first_write() {
        let store = temp_store();
        assert!(!store.dir().exists(), "the store directory already existed");

        store.put(&name("a.json"), b"hello").unwrap();

        assert_eq!(store.get(&name("a.json")).unwrap().unwrap(), b"hello");
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn put_new_creates_the_blob() {
        let store = temp_store();
        store.put_new(&name("a.json"), b"hello").unwrap();
        assert_eq!(store.get(&name("a.json")).unwrap().unwrap(), b"hello");
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn put_new_refuses_a_name_that_is_taken() {
        let store = temp_store();
        store.put(&name("a.json"), b"first").unwrap();

        let err = store.put_new(&name("a.json"), b"second").unwrap_err();

        assert!(matches!(err, StoreError::AlreadyExists(_)), "got {err:?}");
        assert_eq!(store.get(&name("a.json")).unwrap().unwrap(), b"first");
        fs::remove_dir_all(store.dir()).ok();
    }

    /// The first save of a store is where the directory does not exist yet;
    /// `put_new` is what writes the key blob there, so it needs the same
    /// `create_dir_all` `put` has.
    #[test]
    fn put_new_creates_the_store_directory() {
        let store = temp_store();
        assert!(!store.dir().exists(), "the store directory already existed");

        store.put_new(&name("a.json"), b"hello").unwrap();

        assert_eq!(store.get(&name("a.json")).unwrap().unwrap(), b"hello");
        fs::remove_dir_all(store.dir()).ok();
    }

    /// `put_new` is write-then-link, and like `put` it must not leave its
    /// scratch file in the store — after a success, where the link leaves two
    /// names for one file, or after a refusal.
    #[test]
    fn put_new_leaves_no_temp_file_behind() {
        let store = temp_store();
        store.put_new(&name("a.json"), b"first").unwrap();
        assert!(store.put_new(&name("a.json"), b"second").is_err());

        assert_eq!(store.list().unwrap(), [name("a.json")]);
        assert!(
            !fs::read_dir(store.dir()).unwrap().any(|f| f
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(TEMP_EXT)),
            "a scratch file was left in the store"
        );
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

    /// P1-4 guard (green before and after): `put` is a write-then-rename, and
    /// what that must never do is leave its scratch file in the store — where
    /// `list` would report it as a blob and the vault above would try to
    /// decrypt half an entry.
    #[test]
    fn put_leaves_no_temp_file_behind() {
        let store = temp_store();
        store.put(&name("a.json"), b"first").unwrap();
        store.put(&name("a.json"), b"second").unwrap();

        assert_eq!(store.list().unwrap(), [name("a.json")]);
        assert!(
            !fs::read_dir(store.dir()).unwrap().any(|f| f
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(TEMP_EXT)),
            "a scratch file was left in the store"
        );
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn list_skips_scratch_left_by_an_interrupted_put() {
        // A `put` that died between its write and its rename: the bytes are on
        // disk under a name no caller asked for. "Nothing was written" is the
        // only honest answer, since the rename never happened.
        let store = temp_store();
        store.put(&name("a.json"), b"1").unwrap();
        fs::write(store.dir().join(format!("b.json.999-0{TEMP_EXT}")), b"half").unwrap();

        assert_eq!(store.list().unwrap(), [name("a.json")]);
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
