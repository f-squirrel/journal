use journal_store::{BlobName, BlobStore, StoreError};

/// A [`BlobStore`] that says so when it is written to.
///
/// The app hands this to `Vault`, and its callback tells the sync engine which
/// blob just changed (`SyncEngine::mark_dirty`) and wakes the worker. That is
/// how a save reaches the sync machinery without the vault knowing sync exists.
///
/// The callback runs after each *successful* `put`, `put_new` or `delete`, on the calling
/// thread, so it must be quick and must not block on anything the caller could
/// be holding. Reads never notify.
///
/// The engine itself must be given the store *underneath* this wrapper: its
/// own pulls are not local edits, and routing them through here would mark every
/// downloaded blob as waiting to be uploaded.
pub struct NotifyingStore<S> {
    inner: S,
    on_change: Box<dyn Fn(&BlobName) + Send + Sync>,
}

impl<S: BlobStore> NotifyingStore<S> {
    pub fn new(inner: S, on_change: impl Fn(&BlobName) + Send + Sync + 'static) -> Self {
        Self {
            inner,
            on_change: Box::new(on_change),
        }
    }
}

impl<S: BlobStore> BlobStore for NotifyingStore<S> {
    fn get(&self, name: &BlobName) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(name)
    }

    fn put(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
        self.inner.put(name, bytes)?;
        (self.on_change)(name);
        Ok(())
    }

    fn put_new(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
        // Forwarded, not defaulted: the default is `contains` + `put`, and
        // the race it documents is the one the caller used `put_new` to
        // avoid. A successful create is a local change like any other `put`.
        self.inner.put_new(name, bytes)?;
        (self.on_change)(name);
        Ok(())
    }

    fn delete(&self, name: &BlobName) -> Result<(), StoreError> {
        self.inner.delete(name)?;
        (self.on_change)(name);
        Ok(())
    }

    fn list(&self) -> Result<Vec<BlobName>, StoreError> {
        self.inner.list()
    }

    fn contains(&self, name: &BlobName) -> Result<bool, StoreError> {
        self.inner.contains(name)
    }
}
