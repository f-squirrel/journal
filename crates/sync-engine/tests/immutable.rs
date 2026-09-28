//! The engine's shortcut for blobs the app declares immutable: no read, no
//! hash, for one that is here, has a base and is unchanged remotely.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use journal_fs_store::FsStore;
use journal_remote::{MemoryRemote, RemoteStore};
use journal_store::{BlobName, BlobStore, StoreError};
use journal_sync_engine::{SyncEngine, SyncReport};

/// Counts `get` calls for names ending in `.img`.
struct Counting {
    inner: FsStore,
    gets: Arc<AtomicUsize>,
}

impl BlobStore for Counting {
    fn get(&self, name: &BlobName) -> Result<Option<Vec<u8>>, StoreError> {
        if name.as_str().ends_with(".img") {
            self.gets.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.get(name)
    }
    fn put(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
        self.inner.put(name, bytes)
    }
    fn put_new(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
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

struct Rig {
    dir: TempDir,
    store: FsStore,
    remote: Arc<MemoryRemote>,
    gets: Arc<AtomicUsize>,
}

impl Rig {
    fn new() -> Self {
        let dir = TempDir::new();
        let store = FsStore::new(dir.path().join("store"));
        Self {
            dir,
            store,
            remote: Arc::new(MemoryRemote::new()),
            gets: Arc::default(),
        }
    }

    /// `.img` blobs are immutable when `immutable` is set.
    fn open(&self, immutable: bool) -> SyncEngine {
        let local = Counting {
            inner: self.store.clone(),
            gets: self.gets.clone(),
        };
        SyncEngine::open(
            Arc::new(local),
            self.remote.clone(),
            self.dir.path().join("state.json"),
            Box::new(move |name| immutable && name.as_str().ends_with(".img")),
            Box::new(|_| {}),
        )
        .unwrap()
    }

    fn put(&self, name: &str, bytes: &[u8]) {
        self.store.put(&n(name), bytes).unwrap();
    }

    fn gets(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }
}

#[test]
fn an_immutable_blob_with_a_base_is_not_read_on_a_second_pass() {
    let rig = Rig::new();
    let engine = rig.open(true);
    rig.put("a.img", b"pixels");
    engine.sync_once(&NoConflicts).unwrap();
    let after_first = rig.gets();

    let report = engine.sync_once(&NoConflicts).unwrap();

    assert_eq!(report, SyncReport::default());
    assert_eq!(rig.gets(), after_first);
}

#[test]
fn an_immutable_blob_without_a_base_is_read_and_pushed() {
    let rig = Rig::new();
    let engine = rig.open(true);
    rig.put("a.img", b"pixels");

    let report = engine.sync_once(&NoConflicts).unwrap();

    assert_eq!(report.pushed, 1);
    assert!(rig.gets() > 0);
    assert!(rig.remote.get(&n("a.img")).unwrap().is_some());
}

#[test]
fn a_mutable_blob_is_still_read_every_pass() {
    let rig = Rig::new();
    let engine = rig.open(false);
    rig.put("a.img", b"pixels");
    engine.sync_once(&NoConflicts).unwrap();
    let after_first = rig.gets();

    engine.sync_once(&NoConflicts).unwrap();

    assert!(rig.gets() > after_first);
}

#[test]
fn a_deleted_immutable_blob_is_still_pushed_as_a_deletion() {
    let rig = Rig::new();
    let engine = rig.open(true);
    rig.put("a.img", b"pixels");
    engine.sync_once(&NoConflicts).unwrap();

    rig.store.delete(&n("a.img")).unwrap();
    let report = engine.sync_once(&NoConflicts).unwrap();

    assert_eq!(report.pushed, 1);
    assert!(rig.remote.get(&n("a.img")).unwrap().is_none());
}

#[test]
fn open_does_not_read_immutable_blobs_that_have_a_base() {
    let rig = Rig::new();
    rig.open(true).sync_once(&NoConflicts).unwrap();
    rig.put("a.img", b"pixels");
    rig.open(true).sync_once(&NoConflicts).unwrap();
    let before = rig.gets();

    let engine = rig.open(true);

    assert_eq!(rig.gets(), before);
    assert!(engine.status().pending.is_empty());
}
