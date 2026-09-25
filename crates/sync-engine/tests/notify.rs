mod common;

use std::sync::{Arc, Mutex};

use common::TempDir;
use journal_fs_store::FsStore;
use journal_store::{BlobName, BlobStore, StoreError};
use journal_sync_engine::NotifyingStore;

fn n(name: &str) -> BlobName {
    BlobName::try_from(name).unwrap()
}

fn recorded() -> (Arc<Mutex<Vec<String>>>, impl Fn(&BlobName) + Send + Sync) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    (seen, move |name: &BlobName| {
        sink.lock().unwrap().push(name.to_string())
    })
}

#[test]
fn writes_and_deletes_are_reported_by_name_and_reads_are_not() {
    let (seen, on_change) = recorded();
    let dir = TempDir::new();
    let store = NotifyingStore::new(FsStore::new(dir.path()), on_change);

    store.put(&n("a.json"), b"1").unwrap();
    store.put(&n("b.json"), b"2").unwrap();
    store.delete(&n("a.json")).unwrap();
    assert_eq!(*seen.lock().unwrap(), ["a.json", "b.json", "a.json"]);

    // Reads pass straight through.
    seen.lock().unwrap().clear();
    assert_eq!(store.get(&n("b.json")).unwrap().unwrap(), b"2");
    assert_eq!(store.get(&n("gone.json")).unwrap(), None);
    assert!(store.contains(&n("b.json")).unwrap());
    assert_eq!(store.list().unwrap(), [n("b.json")]);
    assert!(seen.lock().unwrap().is_empty());
}

/// An `FsStore` that counts the `put_new` calls that reach it. The trait's
/// default `put_new` would create, refuse and notify just the same through
/// `contains` + `put`, so only the count tells a forward from the racy default.
struct CountingNew {
    inner: FsStore,
    calls: Arc<Mutex<usize>>,
}

impl BlobStore for CountingNew {
    fn get(&self, name: &BlobName) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(name)
    }
    fn put(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
        self.inner.put(name, bytes)
    }
    fn put_new(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
        *self.calls.lock().unwrap() += 1;
        self.inner.put_new(name, bytes)
    }
    fn delete(&self, name: &BlobName) -> Result<(), StoreError> {
        self.inner.delete(name)
    }
    fn list(&self) -> Result<Vec<BlobName>, StoreError> {
        self.inner.list()
    }
    fn contains(&self, name: &BlobName) -> Result<bool, StoreError> {
        self.inner.contains(name)
    }
}

#[test]
fn notifying_store_forwards_put_new() {
    let (seen, on_change) = recorded();
    let dir = TempDir::new();
    let calls = Arc::new(Mutex::new(0));
    let store = NotifyingStore::new(
        CountingNew {
            inner: FsStore::new(dir.path()),
            calls: calls.clone(),
        },
        on_change,
    );

    store.put_new(&n("a.json"), b"1").unwrap();
    assert_eq!(store.get(&n("a.json")).unwrap().unwrap(), b"1");
    assert_eq!(*seen.lock().unwrap(), ["a.json"]);
    assert_eq!(*calls.lock().unwrap(), 1, "put_new did not reach the store");

    // Refused by the inner store, not by a check made here: the blob is
    // untouched and, since nothing changed, nothing is reported.
    let err = store.put_new(&n("a.json"), b"2").unwrap_err();
    assert!(matches!(err, StoreError::AlreadyExists(_)), "got {err:?}");
    assert_eq!(store.get(&n("a.json")).unwrap().unwrap(), b"1");
    assert_eq!(*seen.lock().unwrap(), ["a.json"]);
    assert_eq!(*calls.lock().unwrap(), 2);
}

/// A store whose writes all fail.
struct Broken;

impl BlobStore for Broken {
    fn get(&self, _: &BlobName) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(None)
    }
    fn put(&self, _: &BlobName, _: &[u8]) -> Result<(), StoreError> {
        Err(std::io::Error::other("disk full").into())
    }
    fn delete(&self, _: &BlobName) -> Result<(), StoreError> {
        Err(std::io::Error::other("read-only").into())
    }
    fn list(&self) -> Result<Vec<BlobName>, StoreError> {
        Ok(Vec::new())
    }
    fn contains(&self, _: &BlobName) -> Result<bool, StoreError> {
        Ok(false)
    }
}

#[test]
fn a_write_that_failed_is_not_reported() {
    let (seen, on_change) = recorded();
    let store = NotifyingStore::new(Broken, on_change);

    assert!(store.put(&n("a.json"), b"1").is_err());
    assert!(store.delete(&n("a.json")).is_err());
    assert!(seen.lock().unwrap().is_empty());
}
