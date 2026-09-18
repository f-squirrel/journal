//! An in-memory [`RemoteStore`], for testing whatever sits on top of the trait.
//!
//! It is a faithful stand-in rather than a convenient one: revisions and
//! preconditions are enforced, the change feed reports deletions, feeds page,
//! and the two ways a real provider loses a write race can both be simulated.
//! It can also be told to fail, so a caller's retry and abort paths can be
//! driven without a network.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, MutexGuard};

use journal_store::BlobName;
use sha2::{Digest, Sha256};

use crate::{Change, ChangePage, Cursor, Precondition, RemoteError, RemoteStore, Revision};

/// How the remote loses a write race (D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictMode {
    /// The write is refused: nothing changes and the error carries no bytes.
    /// Dropbox and CloudKit.
    #[default]
    Atomic,
    /// The write lands, and *then* the race is reported, with the overwritten
    /// version in the error. Drive.
    DetectAfterWrite,
}

const DEFAULT_PAGE_SIZE: usize = 1000;

struct Inner {
    blobs: BTreeMap<BlobName, (Vec<u8>, Revision)>,
    /// Every change ever made, in order. The feed cursor is a position in it.
    log: Vec<Change>,
    next_rev: u64,
    mode: ConflictMode,
    /// Queued failures: the front one fires once its count of calls that are
    /// still to succeed reaches zero, then the next one starts counting.
    faults: VecDeque<(usize, RemoteError)>,
}

/// A [`RemoteStore`] that lives in memory.
///
/// Revisions are the decimal write counter, so they are unique and grow with
/// every write; callers must still treat them as opaque, as the trait says.
pub struct MemoryRemote {
    inner: Mutex<Inner>,
    page_size: usize,
    hashing: bool,
}

impl Default for MemoryRemote {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryRemote {
    /// An empty remote with atomic conflicts, a hash for every blob, and pages
    /// large enough that nothing pages.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                blobs: BTreeMap::new(),
                log: Vec::new(),
                next_rev: 1,
                mode: ConflictMode::Atomic,
                faults: VecDeque::new(),
            }),
            page_size: DEFAULT_PAGE_SIZE,
            hashing: true,
        }
    }

    pub fn with_conflict_mode(self, mode: ConflictMode) -> Self {
        self.set_conflict_mode(mode);
        self
    }

    /// At most `size` changes per page, so a test can make paging happen.
    pub fn with_page_size(mut self, size: usize) -> Self {
        assert!(size > 0, "a page must hold at least one change");
        self.page_size = size;
        self
    }

    /// Behave like a backend with no content hash (CloudKit): `content_hash`
    /// returns `None` and the feed carries none.
    pub fn without_content_hash(mut self) -> Self {
        self.hashing = false;
        self
    }

    pub fn set_conflict_mode(&self, mode: ConflictMode) {
        self.lock().mode = mode;
    }

    /// Fails the next call to `changes`, `get`, `put` or `delete` with `error`,
    /// leaving the remote untouched.
    pub fn fail_next(&self, error: RemoteError) {
        self.fail_after(0, error);
    }

    /// Lets `calls` more calls succeed, then fails the one after with `error`.
    ///
    /// Queued failures fire in order, each counting from the previous one, so
    /// this can break a pass at a chosen point. Failing every call takes a
    /// `fail_next` per call.
    pub fn fail_after(&self, calls: usize, error: RemoteError) {
        self.lock().faults.push_back((calls, error));
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("MemoryRemote mutex poisoned")
    }

    /// Takes the lock for one trait call, unless a queued failure is due.
    fn begin(&self) -> Result<MutexGuard<'_, Inner>, RemoteError> {
        let mut inner = self.lock();
        match inner.faults.front_mut() {
            Some((0, _)) => {
                let (_, error) = inner.faults.pop_front().expect("front was just seen");
                return Err(error);
            }
            Some((remaining, _)) => *remaining -= 1,
            None => {}
        }
        Ok(inner)
    }

    fn hash(&self, bytes: &[u8]) -> Option<String> {
        self.hashing.then(|| format!("{:x}", Sha256::digest(bytes)))
    }

    /// Stores the bytes under a fresh revision and logs the change.
    fn write(&self, inner: &mut Inner, name: &BlobName, bytes: &[u8]) -> Revision {
        let rev = Revision::new(inner.next_rev.to_string());
        inner.next_rev += 1;
        inner
            .blobs
            .insert(name.clone(), (bytes.to_vec(), rev.clone()));
        inner.log.push(Change::Upserted {
            name: name.clone(),
            rev: rev.clone(),
            content_hash: self.hash(bytes),
        });
        rev
    }

    fn remove(inner: &mut Inner, name: &BlobName) {
        inner.blobs.remove(name);
        inner.log.push(Change::Deleted { name: name.clone() });
    }

    /// The initial listing, `after` being the last name of the previous page.
    ///
    /// `snapshot` is the feed position when the listing began. The last page
    /// hands back that position, so anything that changed while the listing was
    /// being paged is picked up by the next feed read rather than lost.
    fn list_page(&self, inner: &Inner, snapshot: usize, after: &str) -> ChangePage {
        let mut rest = inner.blobs.iter().filter(|(name, _)| name.as_str() > after);
        let changes: Vec<Change> = rest
            .by_ref()
            .take(self.page_size)
            .map(|(name, (bytes, rev))| Change::Upserted {
                name: name.clone(),
                rev: rev.clone(),
                content_hash: self.hash(bytes),
            })
            .collect();
        let has_more = rest.next().is_some();
        let cursor = match (has_more, changes.last()) {
            (true, Some(last)) => Cursor::new(format!("list:{snapshot}:{}", last.name())),
            _ => Cursor::new(format!("feed:{snapshot}")),
        };
        ChangePage {
            changes,
            cursor,
            has_more,
        }
    }

    fn feed_page(&self, inner: &Inner, from: usize) -> Result<ChangePage, RemoteError> {
        if from > inner.log.len() {
            return Err(RemoteError::Other(format!(
                "cursor at {from} is past the end of the feed ({})",
                inner.log.len()
            )));
        }
        let end = inner.log.len().min(from + self.page_size);
        Ok(ChangePage {
            changes: inner.log[from..end].to_vec(),
            cursor: Cursor::new(format!("feed:{end}")),
            has_more: end < inner.log.len(),
        })
    }
}

fn bad_cursor(cursor: &Cursor) -> RemoteError {
    RemoteError::Other(format!("unrecognised cursor {:?}", cursor.as_str()))
}

impl RemoteStore for MemoryRemote {
    fn changes(&self, since: Option<&Cursor>) -> Result<ChangePage, RemoteError> {
        let inner = self.begin()?;
        let Some(cursor) = since else {
            return Ok(self.list_page(&inner, inner.log.len(), ""));
        };
        // Cursors are `feed:<position>` or `list:<snapshot>:<last name>`. A
        // blob name cannot contain `:`, so the split is unambiguous.
        match cursor.as_str().split(':').collect::<Vec<_>>()[..] {
            ["feed", from] => {
                let from = from.parse().map_err(|_| bad_cursor(cursor))?;
                self.feed_page(&inner, from)
            }
            ["list", snapshot, after] => {
                let snapshot = snapshot.parse().map_err(|_| bad_cursor(cursor))?;
                Ok(self.list_page(&inner, snapshot, after))
            }
            _ => Err(bad_cursor(cursor)),
        }
    }

    fn get(&self, name: &BlobName) -> Result<Option<(Vec<u8>, Revision)>, RemoteError> {
        Ok(self.begin()?.blobs.get(name).cloned())
    }

    fn put(
        &self,
        name: &BlobName,
        bytes: &[u8],
        pre: Precondition,
    ) -> Result<Revision, RemoteError> {
        let mut inner = self.begin()?;
        let existing = inner.blobs.get(name).cloned();
        let holds = match (&pre, &existing) {
            (Precondition::Absent, None) => true,
            (Precondition::Matches(rev), Some((_, current))) => rev == current,
            _ => false,
        };
        if holds {
            return Ok(self.write(&mut inner, name, bytes));
        }
        match (inner.mode, existing) {
            (ConflictMode::DetectAfterWrite, Some(theirs)) => {
                self.write(&mut inner, name, bytes);
                Err(RemoteError::Conflict {
                    theirs: Some(theirs),
                })
            }
            // Atomic, or a `Matches` on a blob that is gone: there is nothing
            // to overwrite, and a real backend cannot update a vanished blob.
            _ => Err(RemoteError::Conflict { theirs: None }),
        }
    }

    fn delete(&self, name: &BlobName, pre: Precondition) -> Result<(), RemoteError> {
        let mut inner = self.begin()?;
        let Some(existing) = inner.blobs.get(name).cloned() else {
            return Ok(());
        };
        if matches!(&pre, Precondition::Matches(rev) if *rev == existing.1) {
            Self::remove(&mut inner, name);
            return Ok(());
        }
        match inner.mode {
            ConflictMode::Atomic => Err(RemoteError::Conflict { theirs: None }),
            ConflictMode::DetectAfterWrite => {
                Self::remove(&mut inner, name);
                Err(RemoteError::Conflict {
                    theirs: Some(existing),
                })
            }
        }
    }

    fn content_hash(&self, bytes: &[u8]) -> Option<String> {
        self.hash(bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn name(s: &str) -> BlobName {
        BlobName::try_from(s).unwrap()
    }

    fn rev_number(rev: &Revision) -> u64 {
        rev.as_str().parse().unwrap()
    }

    fn put(remote: &MemoryRemote, n: &str, bytes: &[u8]) -> Revision {
        remote.put(&name(n), bytes, Precondition::Absent).unwrap()
    }

    /// Every name and its bytes, as a full listing reports it.
    fn listing(remote: &MemoryRemote) -> Vec<String> {
        let mut out = Vec::new();
        let mut page = remote.changes(None).unwrap();
        loop {
            out.extend(page.changes.iter().map(|c| c.name().to_string()));
            if !page.has_more {
                return out;
            }
            page = remote.changes(Some(&page.cursor)).unwrap();
        }
    }

    #[test]
    fn revisions_increase_with_every_write() {
        let remote = MemoryRemote::new();
        let a = put(&remote, "a", b"1");
        let b = put(&remote, "b", b"1");
        let a2 = remote
            .put(&name("a"), b"2", Precondition::Matches(a.clone()))
            .unwrap();
        assert!(rev_number(&a) < rev_number(&b));
        assert!(rev_number(&b) < rev_number(&a2));
        assert_eq!(remote.get(&name("a")).unwrap(), Some((b"2".to_vec(), a2)));
    }

    #[test]
    fn get_of_a_missing_blob_is_none() {
        assert_eq!(MemoryRemote::new().get(&name("a")).unwrap(), None);
    }

    #[test]
    fn absent_creates_only_and_refuses_an_existing_blob() {
        let remote = MemoryRemote::new();
        let rev = put(&remote, "a", b"first");
        let err = remote
            .put(&name("a"), b"second", Precondition::Absent)
            .unwrap_err();
        assert_eq!(err, RemoteError::Conflict { theirs: None });
        assert_eq!(
            remote.get(&name("a")).unwrap(),
            Some((b"first".to_vec(), rev))
        );
    }

    #[test]
    fn matches_replaces_only_at_the_named_revision() {
        let remote = MemoryRemote::new();
        let stale = put(&remote, "a", b"1");
        let current = remote
            .put(&name("a"), b"2", Precondition::Matches(stale.clone()))
            .unwrap();

        let err = remote
            .put(&name("a"), b"3", Precondition::Matches(stale))
            .unwrap_err();
        assert_eq!(err, RemoteError::Conflict { theirs: None });
        assert_eq!(
            remote.get(&name("a")).unwrap(),
            Some((b"2".to_vec(), current))
        );
    }

    #[test]
    fn matches_on_a_blob_that_is_gone_is_a_conflict_in_both_modes() {
        for mode in [ConflictMode::Atomic, ConflictMode::DetectAfterWrite] {
            let remote = MemoryRemote::new().with_conflict_mode(mode);
            let rev = put(&remote, "a", b"1");
            remote
                .delete(&name("a"), Precondition::Matches(rev.clone()))
                .unwrap();

            let err = remote
                .put(&name("a"), b"2", Precondition::Matches(rev))
                .unwrap_err();
            assert_eq!(err, RemoteError::Conflict { theirs: None }, "{mode:?}");
            assert_eq!(remote.get(&name("a")).unwrap(), None, "{mode:?}");
        }
    }

    #[test]
    fn delete_is_conditional_and_idempotent() {
        let remote = MemoryRemote::new();
        let stale = put(&remote, "a", b"1");
        let current = remote
            .put(&name("a"), b"2", Precondition::Matches(stale.clone()))
            .unwrap();

        let err = remote
            .delete(&name("a"), Precondition::Matches(stale.clone()))
            .unwrap_err();
        assert_eq!(err, RemoteError::Conflict { theirs: None });
        assert!(remote.get(&name("a")).unwrap().is_some());

        remote
            .delete(&name("a"), Precondition::Matches(current))
            .unwrap();
        assert_eq!(remote.get(&name("a")).unwrap(), None);

        // Already gone: the caller wanted it gone, whatever it thought it was.
        remote
            .delete(&name("a"), Precondition::Matches(stale))
            .unwrap();
        remote.delete(&name("never"), Precondition::Absent).unwrap();
    }

    #[test]
    fn detect_after_write_lands_the_write_and_returns_theirs() {
        let remote = MemoryRemote::new().with_conflict_mode(ConflictMode::DetectAfterWrite);
        let theirs = put(&remote, "a", b"theirs");

        let err = remote
            .put(&name("a"), b"ours", Precondition::Absent)
            .unwrap_err();
        assert_eq!(
            err,
            RemoteError::Conflict {
                theirs: Some((b"theirs".to_vec(), theirs.clone()))
            }
        );
        // A plain `get` now returns our own bytes: exactly why `theirs` exists.
        let (bytes, rev) = remote.get(&name("a")).unwrap().unwrap();
        assert_eq!(bytes, b"ours");
        assert!(rev_number(&rev) > rev_number(&theirs));
    }

    #[test]
    fn detect_after_write_delete_lands_and_returns_theirs() {
        let remote = MemoryRemote::new().with_conflict_mode(ConflictMode::DetectAfterWrite);
        let stale = put(&remote, "a", b"1");
        let theirs = remote
            .put(&name("a"), b"2", Precondition::Matches(stale.clone()))
            .unwrap();

        let err = remote
            .delete(&name("a"), Precondition::Matches(stale))
            .unwrap_err();
        assert_eq!(
            err,
            RemoteError::Conflict {
                theirs: Some((b"2".to_vec(), theirs))
            }
        );
        assert_eq!(remote.get(&name("a")).unwrap(), None);
    }

    #[test]
    fn an_atomic_conflict_changes_nothing_and_logs_nothing() {
        let remote = MemoryRemote::new();
        put(&remote, "a", b"1");
        let cursor = remote.changes(None).unwrap().cursor;

        remote
            .put(&name("a"), b"2", Precondition::Absent)
            .unwrap_err();
        remote.delete(&name("a"), Precondition::Absent).unwrap_err();

        assert!(remote.changes(Some(&cursor)).unwrap().changes.is_empty());
    }

    #[test]
    fn the_initial_listing_reports_every_blob_and_no_deletions() {
        let remote = MemoryRemote::new();
        put(&remote, "b", b"1");
        put(&remote, "a", b"1");
        let gone = put(&remote, "gone", b"1");
        remote
            .delete(&name("gone"), Precondition::Matches(gone))
            .unwrap();

        let page = remote.changes(None).unwrap();
        assert!(!page.has_more);
        assert!(page
            .changes
            .iter()
            .all(|c| matches!(c, Change::Upserted { .. })));
        assert_eq!(listing(&remote), ["a", "b"]);
    }

    #[test]
    fn the_initial_listing_of_an_empty_remote_is_empty_and_resumable() {
        let remote = MemoryRemote::new();
        let page = remote.changes(None).unwrap();
        assert!(page.changes.is_empty() && !page.has_more);

        put(&remote, "a", b"1");
        let next = remote.changes(Some(&page.cursor)).unwrap();
        assert_eq!(next.changes.len(), 1);
    }

    #[test]
    fn the_feed_reports_changes_since_the_cursor_including_deletions() {
        let remote = MemoryRemote::new();
        let a = put(&remote, "a", b"1");
        let first = remote.changes(None).unwrap();

        remote
            .put(&name("a"), b"2", Precondition::Matches(a))
            .unwrap();
        let b = put(&remote, "b", b"1");
        remote.delete(&name("b"), Precondition::Matches(b)).unwrap();

        let page = remote.changes(Some(&first.cursor)).unwrap();
        assert!(!page.has_more);
        let summary: Vec<_> = page
            .changes
            .iter()
            .map(|c| (c.name().to_string(), matches!(c, Change::Deleted { .. })))
            .collect();
        assert_eq!(
            summary,
            [
                ("a".to_string(), false),
                ("b".to_string(), false),
                ("b".to_string(), true)
            ]
        );

        // Nothing new since the page's own cursor.
        assert!(remote
            .changes(Some(&page.cursor))
            .unwrap()
            .changes
            .is_empty());
    }

    #[test]
    fn a_feed_pages_and_the_last_cursor_resumes_cleanly() {
        let remote = MemoryRemote::new().with_page_size(2);
        let start = remote.changes(None).unwrap().cursor;
        for n in ["a", "b", "c", "d", "e"] {
            put(&remote, n, b"1");
        }

        let mut seen = Vec::new();
        let mut page = remote.changes(Some(&start)).unwrap();
        let mut pages = 1;
        loop {
            assert!(page.changes.len() <= 2);
            seen.extend(page.changes.iter().map(|c| c.name().to_string()));
            if !page.has_more {
                break;
            }
            page = remote.changes(Some(&page.cursor)).unwrap();
            pages += 1;
        }
        assert_eq!(seen, ["a", "b", "c", "d", "e"]);
        assert_eq!(pages, 3);
        assert!(remote
            .changes(Some(&page.cursor))
            .unwrap()
            .changes
            .is_empty());
    }

    #[test]
    fn the_initial_listing_pages_too() {
        let remote = MemoryRemote::new().with_page_size(2);
        for n in ["e", "d", "c", "b", "a"] {
            put(&remote, n, b"1");
        }
        assert_eq!(listing(&remote), ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn a_write_during_a_paged_listing_is_not_lost() {
        let remote = MemoryRemote::new().with_page_size(2);
        for n in ["a", "b", "c", "d"] {
            put(&remote, n, b"1");
        }
        let first = remote.changes(None).unwrap();
        assert!(first.has_more);

        // Behind the listing (`a` was already sent) and ahead of it (`z`).
        let a = remote.get(&name("a")).unwrap().unwrap().1;
        remote
            .put(&name("a"), b"2", Precondition::Matches(a))
            .unwrap();
        put(&remote, "z", b"1");

        let mut page = first;
        while page.has_more {
            page = remote.changes(Some(&page.cursor)).unwrap();
        }
        // The final cursor is the position the listing began at, so the feed
        // replays everything that happened while it was paging.
        let replay = remote.changes(Some(&page.cursor)).unwrap();
        let replayed: Vec<_> = replay.changes.iter().map(|c| c.name().as_str()).collect();
        assert_eq!(replayed, ["a", "z"]);
    }

    #[test]
    fn an_unrecognised_cursor_is_an_error_not_a_full_listing() {
        let remote = MemoryRemote::new();
        for bad in ["", "nope", "feed:x", "feed:99", "list:1"] {
            let err = remote.changes(Some(&Cursor::new(bad))).unwrap_err();
            assert!(matches!(err, RemoteError::Other(_)), "{bad:?}: {err:?}");
        }
    }

    #[test]
    fn content_hash_is_sha256_and_matches_what_the_feed_reports() {
        let remote = MemoryRemote::new();
        assert_eq!(
            remote.content_hash(b"abc").as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );

        put(&remote, "a", b"abc");
        let page = remote.changes(None).unwrap();
        let Change::Upserted { content_hash, .. } = &page.changes[0] else {
            panic!("expected an upsert");
        };
        assert_eq!(*content_hash, remote.content_hash(b"abc"));
    }

    #[test]
    fn a_remote_without_a_hash_reports_none() {
        let remote = MemoryRemote::new().without_content_hash();
        assert_eq!(remote.content_hash(b"abc"), None);

        put(&remote, "a", b"abc");
        assert!(matches!(
            remote.changes(None).unwrap().changes[0],
            Change::Upserted {
                content_hash: None,
                ..
            }
        ));
    }

    #[test]
    fn an_injected_failure_fires_once_and_leaves_the_remote_untouched() {
        let remote = MemoryRemote::new();
        remote.fail_next(RemoteError::Unauthorized);

        let err = remote
            .put(&name("a"), b"1", Precondition::Absent)
            .unwrap_err();
        assert_eq!(err, RemoteError::Unauthorized);
        assert_eq!(remote.get(&name("a")).unwrap(), None);

        put(&remote, "a", b"1");
    }

    #[test]
    fn failures_can_be_placed_partway_through_and_queued() {
        let remote = MemoryRemote::new();
        let limited = RemoteError::RateLimited {
            retry_after: Duration::from_secs(3),
        };
        remote.fail_after(2, limited.clone());
        remote.fail_after(0, RemoteError::Transient("offline".into()));

        put(&remote, "a", b"1");
        put(&remote, "b", b"1");
        let err = remote
            .put(&name("c"), b"1", Precondition::Absent)
            .unwrap_err();
        assert_eq!(err, limited);
        // The second one was waiting behind the first, and is due now.
        let err = remote.get(&name("a")).unwrap_err();
        assert_eq!(err, RemoteError::Transient("offline".into()));

        assert_eq!(listing(&remote), ["a", "b"]);
    }
}
