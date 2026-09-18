mod common;

use std::sync::{Arc, Mutex};

use common::TempDir;
use journal_fs_store::FsStore;
use journal_store::{BlobName, BlobStore, StoreError};
use journal_sync::NotifyingStore;

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
