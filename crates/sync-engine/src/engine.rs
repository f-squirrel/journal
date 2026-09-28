use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use journal_remote::RemoteStore;
use journal_store::{BlobName, BlobStore};

use crate::digest::Digest;
use crate::pass::Pass;
use crate::state::State;
use crate::{SyncError, SyncStatus};

/// The name of the sealed key blob, which is never merged (D5).
///
/// Must equal the name `journal-key` writes it under. That crate keeps its
/// constant private, and this one cannot depend on it — the engine holds no key
/// — so a test in this crate creates a real key and checks the two agree.
pub const KEY_BLOB: &str = ".vault-key";

/// What the engine asks of whoever can read entries.
///
/// The engine moves ciphertext and cannot tell two versions of an entry apart
/// beyond "different". Deciding what a conflict means needs plaintext, so it is
/// a hook implemented one layer up, by the code that holds the key.
pub trait ConflictResolver: Send + Sync {
    /// Called with both versions of one blob, which differ.
    ///
    /// Return [`Resolution::Resolved`] once the merged result has been written
    /// to the local store under `name` — anything else the resolver saves, such
    /// as a conflicted copy under a new name, goes to the same store. The engine
    /// then treats the remote as still holding `theirs` and uploads the result.
    ///
    /// Return [`Resolution::Deferred`] to leave the conflict parked, for
    /// instance while the vault is locked. It is offered again on every later
    /// pass.
    ///
    /// The engine may call this again for a conflict it has already offered,
    /// after a crash between the resolver returning and the engine recording
    /// it. Resolving is therefore at-least-once, not exactly-once.
    fn resolve(&self, name: &BlobName, ours: &[u8], theirs: &[u8]) -> Resolution;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Resolved,
    Deferred,
}

/// Tells the engine which blobs are immutable: see [`SyncEngine::open`].
pub type Immutable = Box<dyn Fn(&BlobName) -> bool + Send + Sync>;

/// What one pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Blobs downloaded and written to the local store.
    pub pulled: usize,
    /// Changes sent to the remote: uploads and deletions.
    pub pushed: usize,
    /// Blobs deleted from the local store because they were deleted remotely.
    pub deleted: usize,
    /// Conflicts offered to the resolver, whether it resolved them or not.
    pub conflicts: usize,
}

/// Replicates blobs between a local store and a remote, and reports what has
/// not yet made it across.
///
/// The engine holds no key and speaks only ciphertext. Retry and scheduling are
/// not here: [`sync_once`](Self::sync_once) returns a typed error and the caller
/// decides what to do about it.
///
/// Give it the store *beneath* any `NotifyingStore`: writes the engine makes
/// itself are not local edits.
pub struct SyncEngine {
    pub(crate) local: Arc<dyn BlobStore>,
    pub(crate) remote: Arc<dyn RemoteStore>,
    immutable: Immutable,
    on_status: Box<dyn Fn(&SyncStatus) + Send + Sync>,
    /// The status, and the counters behind it. Held only for an instant and
    /// never across I/O, because `mark_dirty` arrives from command threads while
    /// a pass is running.
    shared: Mutex<Shared>,
    /// The persisted state. Held for a whole pass, which is also what makes
    /// passes run one at a time.
    state: Mutex<State>,
}

#[derive(Default)]
struct Shared {
    status: SyncStatus,
    /// Bumped by every `mark_dirty`, per name. A pass records it when it reads
    /// a blob and clears the blob's pending mark only if it is unchanged after
    /// the upload: otherwise a save landed in between and the upload carried
    /// the older bytes.
    dirty: HashMap<BlobName, u64>,
}

/// How a pass left one blob, as far as its pending mark goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The local copy and the remote agree, and the base records it.
    Synced,
    /// A local change is still waiting to reach the remote.
    Pending,
    /// Nothing was learned. Leave the mark as it is.
    Skipped,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere must not wedge sync for the rest of the session.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SyncEngine {
    pub(crate) fn is_immutable(&self, name: &BlobName) -> bool {
        (self.immutable)(name)
    }

    /// Opens the engine over `local` and `remote`, keeping its per-device state
    /// in the file at `state_path`.
    ///
    /// Scans the local store against that state, so `pending` is correct from
    /// the start and survives restarts without being persisted itself.
    /// `state_path` must not be inside `local`, or the engine would sync its
    /// own bookkeeping.
    ///
    /// `immutable` names the blobs the app promises never to rewrite with
    /// different content once written (the engine itself knows nothing about
    /// what they are). For such a blob that exists locally and has a recorded
    /// base, the local copy is by definition the base: it is not read or hashed
    /// at startup or in a pass, which is what keeps a journal full of large,
    /// never-changing blobs cheap to sync. A name matching it that *is*
    /// rewritten will not be noticed. Pass `Box::new(|_| false)` for none.
    ///
    /// `on_status` fires on every change to the status and is not
    /// deduplicated. It runs on whichever thread made the change, with no lock
    /// held, so two calls can arrive out of order: treat it as "something
    /// changed" and read [`status`](Self::status) for the latest. It must not
    /// call [`sync_once`](Self::sync_once).
    pub fn open(
        local: Arc<dyn BlobStore>,
        remote: Arc<dyn RemoteStore>,
        state_path: PathBuf,
        immutable: Immutable,
        on_status: Box<dyn Fn(&SyncStatus) + Send + Sync>,
    ) -> Result<Self, SyncError> {
        let state = State::load(state_path)?;

        let mut pending = BTreeSet::new();
        let names: BTreeSet<_> = local.list()?.into_iter().collect();
        for name in &names {
            if (immutable)(name) && state.bases.contains_key(name) {
                continue;
            }
            // Gone since the listing: the next pass will see it as a delete.
            let Some(bytes) = local.get(name)? else {
                continue;
            };
            let synced = state.bases.get(name).map(|b| b.hash) == Some(Digest::of(&bytes));
            if !synced {
                pending.insert(name.clone());
            }
        }
        pending.extend(state.bases.keys().filter(|n| !names.contains(*n)).cloned());
        let conflicted: BTreeSet<_> = state.parked.keys().cloned().collect();
        pending.extend(conflicted.iter().cloned());

        Ok(Self {
            local,
            remote,
            immutable,
            on_status,
            shared: Mutex::new(Shared {
                status: SyncStatus {
                    pending,
                    conflicted,
                    ..SyncStatus::default()
                },
                dirty: HashMap::new(),
            }),
            state: Mutex::new(state),
        })
    }

    /// Runs one sync pass: pulls what changed on the remote, then reconciles
    /// every blob against the state recorded at the last sync.
    ///
    /// Safe to re-run after any failure: the base state is written per blob
    /// after that blob succeeds, and the cursor last. Blocks until done, so
    /// call it from a worker thread. Concurrent calls take turns.
    ///
    /// A resolved conflict can leave more to push — the merged entry and
    /// whatever else the resolver saved — so a pass repeats itself, a few times
    /// at most, while it keeps resolving conflicts.
    pub fn sync_once(&self, resolver: &dyn ConflictResolver) -> Result<SyncReport, SyncError> {
        let mut state = lock(&self.state);
        self.update(|s| {
            s.status.running = true;
            true
        });

        let result = Pass::run(self, &mut state, resolver);

        match &result {
            Ok(report) => log::info!("sync pass done: {report:?}"),
            Err(e) => log::warn!("sync pass failed: {e}"),
        }
        self.update(|s| {
            s.status.running = false;
            match &result {
                Ok(_) => {
                    s.status.last_success = Some(SystemTime::now());
                    s.status.last_error = None;
                }
                Err(e) => s.status.last_error = Some(e.kind()),
            }
            true
        });
        result
    }

    /// Called when a blob has just been written or deleted locally — from
    /// `NotifyingStore`'s callback. Marks it pending at once, before any scan,
    /// so the UI reacts to a save immediately.
    pub fn mark_dirty(&self, name: &BlobName) {
        self.update(|s| {
            *s.dirty.entry(name.clone()).or_insert(0) += 1;
            s.status.pending.insert(name.clone());
            true
        });
    }

    pub fn status(&self) -> SyncStatus {
        lock(&self.shared).status.clone()
    }

    /// Applies `change` to the status and reports it, unless it says nothing
    /// changed. The callback runs after the lock is released.
    fn update(&self, change: impl FnOnce(&mut Shared) -> bool) {
        let snapshot = {
            let mut shared = lock(&self.shared);
            if !change(&mut shared) {
                return;
            }
            shared.status.clone()
        };
        (self.on_status)(&snapshot);
    }

    pub(crate) fn dirty_count(&self, name: &BlobName) -> u64 {
        lock(&self.shared).dirty.get(name).copied().unwrap_or(0)
    }

    /// Applies what a pass learned about one blob to its pending mark.
    ///
    /// `seen` is the dirty counter from before the pass read the blob. A blob
    /// counts as synced only if no save has landed since: the base holds the
    /// hash of the bytes that were actually uploaded, so a newer save simply
    /// shows up as a local change on the next pass.
    pub(crate) fn settle(&self, name: &BlobName, seen: u64, outcome: Outcome) {
        match outcome {
            Outcome::Synced => self.update(|s| {
                s.dirty.get(name).copied().unwrap_or(0) == seen && s.status.pending.remove(name)
            }),
            Outcome::Pending => self.update(|s| s.status.pending.insert(name.clone())),
            Outcome::Skipped => {}
        }
    }

    pub(crate) fn set_conflicted(&self, name: &BlobName, conflicted: bool) {
        self.update(|s| {
            if conflicted {
                s.status.conflicted.insert(name.clone())
            } else {
                s.status.conflicted.remove(name)
            }
        });
    }
}
