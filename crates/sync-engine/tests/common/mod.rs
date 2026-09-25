//! Two-device harness: each `Device` has its own `FsStore` in a temp directory,
//! its own engine and state file, and shares one remote with the others.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use journal_fs_store::FsStore;
use journal_remote::{
    ChangePage, Cursor, MemoryRemote, Precondition, RemoteError, RemoteStore, Revision,
};
use journal_store::{BlobName, BlobStore};
use journal_sync_engine::{
    ConflictResolver, NotifyingStore, Resolution, SyncEngine, SyncError, SyncReport, SyncStatus,
};

pub fn n(name: &str) -> BlobName {
    BlobName::try_from(name).unwrap()
}

/// A directory no other test in this process will pick, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }

    pub fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        Self(std::env::temp_dir().join(format!(
            "journal-sync-engine-test-{}-{n}",
            std::process::id()
        )))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Device {
    dir: TempDir,
    /// The store the engine and the resolver use: writes here are not edits.
    pub store: FsStore,
    /// What the app's vault would write through.
    app: NotifyingStore<FsStore>,
    remote: Arc<dyn RemoteStore>,
    cell: Arc<Mutex<Option<Arc<SyncEngine>>>>,
    pub events: Arc<Mutex<Vec<SyncStatus>>>,
}

impl Device {
    pub fn new(remote: Arc<dyn RemoteStore>) -> Self {
        let dir = TempDir::new();
        let store = FsStore::new(dir.0.join("store"));
        let cell: Arc<Mutex<Option<Arc<SyncEngine>>>> = Arc::default();
        let notify = cell.clone();
        let app = NotifyingStore::new(store.clone(), move |name| {
            if let Some(engine) = notify.lock().unwrap().as_ref() {
                engine.mark_dirty(name);
            }
        });
        let device = Self {
            dir,
            store,
            app,
            remote,
            cell,
            events: Arc::default(),
        };
        device.reopen();
        device
    }

    /// A new engine over the same store and state file, as after a restart.
    pub fn reopen(&self) {
        let events = self.events.clone();
        let engine = SyncEngine::open(
            Arc::new(self.store.clone()),
            self.remote.clone(),
            self.state_path(),
            Box::new(move |status| events.lock().unwrap().push(status.clone())),
        )
        .unwrap();
        *self.cell.lock().unwrap() = Some(Arc::new(engine));
    }

    pub fn engine(&self) -> Arc<SyncEngine> {
        self.cell.lock().unwrap().clone().unwrap()
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.0.join("sync-state.json")
    }

    pub fn state_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.state_path()).ok()
    }

    /// A local edit, the way the app makes one.
    pub fn put(&self, name: &str, bytes: &[u8]) {
        self.app.put(&n(name), bytes).unwrap();
    }

    pub fn delete(&self, name: &str) {
        self.app.delete(&n(name)).unwrap();
    }

    /// A local change the engine is *not* told about.
    pub fn put_quietly(&self, name: &str, bytes: &[u8]) {
        self.store.put(&n(name), bytes).unwrap();
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.store.get(&n(name)).unwrap()
    }

    pub fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        self.store
            .list()
            .unwrap()
            .into_iter()
            .map(|name| {
                let bytes = self.store.get(&name).unwrap().unwrap();
                (name.to_string(), bytes)
            })
            .collect()
    }

    pub fn status(&self) -> SyncStatus {
        self.engine().status()
    }

    /// A pass in which no conflict is expected.
    pub fn sync(&self) -> SyncReport {
        self.try_sync().unwrap()
    }

    pub fn try_sync(&self) -> Result<SyncReport, SyncError> {
        self.try_sync_with(&NoConflicts)
    }

    pub fn sync_with(&self, resolver: &dyn ConflictResolver) -> SyncReport {
        self.try_sync_with(resolver).unwrap()
    }

    pub fn try_sync_with(&self, resolver: &dyn ConflictResolver) -> Result<SyncReport, SyncError> {
        self.engine().sync_once(resolver)
    }

    /// Passes until one succeeds. For tests that have injected a one-shot fault.
    pub fn sync_until_ok(&self) -> SyncReport {
        for _ in 0..3 {
            if let Ok(report) = self.try_sync() {
                return report;
            }
        }
        panic!("sync kept failing");
    }
}

/// Panics if a conflict is offered: for tests where none is expected.
pub struct NoConflicts;

impl ConflictResolver for NoConflicts {
    fn resolve(&self, name: &BlobName, _: &[u8], _: &[u8]) -> Resolution {
        panic!("unexpected conflict on {name}");
    }
}

/// Keeps the larger version under the original name and the other beside it as
/// `<name>.copy`, in the store it was given — the shape of the real resolver.
pub struct KeepBoth {
    store: FsStore,
    pub defer: Mutex<bool>,
    /// Every call, as (name, ours, theirs).
    pub calls: Mutex<Vec<(String, Vec<u8>, Vec<u8>)>>,
}

impl KeepBoth {
    pub fn new(device: &Device) -> Self {
        Self {
            store: device.store.clone(),
            defer: Mutex::new(false),
            calls: Mutex::default(),
        }
    }

    pub fn deferring(device: &Device) -> Self {
        let resolver = Self::new(device);
        *resolver.defer.lock().unwrap() = true;
        resolver
    }

    pub fn calls(&self) -> Vec<(String, Vec<u8>, Vec<u8>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl ConflictResolver for KeepBoth {
    fn resolve(&self, name: &BlobName, ours: &[u8], theirs: &[u8]) -> Resolution {
        self.calls
            .lock()
            .unwrap()
            .push((name.to_string(), ours.to_vec(), theirs.to_vec()));
        if *self.defer.lock().unwrap() {
            return Resolution::Deferred;
        }
        let (winner, loser) = if ours >= theirs {
            (ours, theirs)
        } else {
            (theirs, ours)
        };
        self.store.put(name, winner).unwrap();
        self.store.put(&n(&format!("{name}.copy")), loser).unwrap();
        Resolution::Resolved
    }
}

/// Everything on the remote, name to bytes.
pub fn remote_snapshot(remote: &dyn RemoteStore) -> BTreeMap<String, Vec<u8>> {
    let mut names = Vec::new();
    let mut page = remote.changes(None).unwrap();
    loop {
        names.extend(page.changes.iter().map(|c| c.name().clone()));
        if !page.has_more {
            break;
        }
        page = remote.changes(Some(&page.cursor)).unwrap();
    }
    names
        .into_iter()
        .filter_map(|name| {
            let (bytes, _) = remote.get(&name).unwrap()?;
            Some((name.to_string(), bytes))
        })
        .collect()
}

pub fn snapshot_of(pairs: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
    pairs
        .iter()
        .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
        .collect()
}

type Hook = Mutex<Option<Box<dyn FnOnce() + Send>>>;

/// A remote that runs a closure just before its next `put` or `delete`: the
/// moment another device gets in between a pass reading the remote and writing
/// to it.
pub struct Hooked {
    pub inner: Arc<MemoryRemote>,
    before_put: Hook,
    before_delete: Hook,
}

impl Hooked {
    pub fn new(inner: Arc<MemoryRemote>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            before_put: Mutex::default(),
            before_delete: Mutex::default(),
        })
    }

    pub fn before_next_put(&self, hook: impl FnOnce() + Send + 'static) {
        *self.before_put.lock().unwrap() = Some(Box::new(hook));
    }

    pub fn before_next_delete(&self, hook: impl FnOnce() + Send + 'static) {
        *self.before_delete.lock().unwrap() = Some(Box::new(hook));
    }
}

impl RemoteStore for Hooked {
    fn changes(&self, since: Option<&Cursor>) -> Result<ChangePage, RemoteError> {
        self.inner.changes(since)
    }

    fn get(&self, name: &BlobName) -> Result<Option<(Vec<u8>, Revision)>, RemoteError> {
        self.inner.get(name)
    }

    fn put(
        &self,
        name: &BlobName,
        bytes: &[u8],
        pre: Precondition,
    ) -> Result<Revision, RemoteError> {
        if let Some(hook) = self.before_put.lock().unwrap().take() {
            hook();
        }
        self.inner.put(name, bytes, pre)
    }

    fn delete(&self, name: &BlobName, pre: Precondition) -> Result<(), RemoteError> {
        if let Some(hook) = self.before_delete.lock().unwrap().take() {
            hook();
        }
        self.inner.delete(name, pre)
    }

    fn content_hash(&self, bytes: &[u8]) -> Option<String> {
        self.inner.content_hash(bytes)
    }
}
