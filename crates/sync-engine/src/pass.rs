//! One sync pass: what to do with each blob, given what changed on each side.
//!
//! For every name the pass knows three things: the local bytes (if any), the
//! *base* recorded at the last successful sync, and what the remote did since.
//! "Changed" always means "different from the base", never "newer than the
//! other side" — nothing here compares clocks.

use std::collections::{BTreeMap, BTreeSet};

use journal_remote::{Change, Cursor, Precondition, RemoteError, Revision};
use journal_store::{BlobName, StoreError};

use crate::digest::Digest;
use crate::engine::{Outcome, KEY_BLOB};
use crate::state::{Base, Parked, State};
use crate::{ConflictResolver, Resolution, SyncEngine, SyncError, SyncReport};

/// A pass repeats while it keeps resolving conflicts, because a resolution
/// leaves things to push. This bounds a remote that never stops changing; what
/// is left over stays pending for the next pass.
const MAX_ROUNDS: usize = 4;

/// The last thing the feed said about a name.
#[derive(Clone)]
enum Seen {
    Present { rev: Revision, hash: Option<String> },
    Gone,
}

/// What the remote did since the cursor.
struct RemoteView {
    /// The feed was a full listing, so a name it does not mention is not there.
    /// Otherwise a name it does not mention has not changed.
    complete: bool,
    seen: BTreeMap<BlobName, Seen>,
}

/// What the remote did to one blob, measured against the base.
enum Theirs {
    /// Nothing, or only what our own last push already recorded.
    Same,
    Edited {
        rev: Revision,
        hash: Option<String>,
    },
    Removed,
}

static GONE: Seen = Seen::Gone;

impl RemoteView {
    /// What the feed says about `name`, if it says anything.
    fn seen(&self, name: &BlobName) -> Option<&Seen> {
        match self.seen.get(name) {
            None if self.complete => Some(&GONE),
            seen => seen,
        }
    }

    fn theirs(&self, name: &BlobName, base: Option<&Base>) -> Theirs {
        let Some(seen) = self.seen(name) else {
            return Theirs::Same;
        };
        match (seen, base) {
            (Seen::Present { rev, .. }, Some(base)) if *rev == base.rev => Theirs::Same,
            (Seen::Present { rev, hash }, _) => Theirs::Edited {
                rev: rev.clone(),
                hash: hash.clone(),
            },
            (Seen::Gone, Some(_)) => Theirs::Removed,
            (Seen::Gone, None) => Theirs::Same,
        }
    }
}

struct Local {
    bytes: Vec<u8>,
    digest: Digest,
}

pub(crate) struct Pass<'a> {
    engine: &'a SyncEngine,
    state: &'a mut State,
    resolver: &'a dyn ConflictResolver,
    report: SyncReport,
    /// Something was left to push, so another round is worthwhile.
    again: bool,
    /// Names the resolver has already been asked about in this pass, so a later
    /// round does not offer a deferred conflict a second time.
    offered: BTreeSet<BlobName>,
}

impl<'a> Pass<'a> {
    pub(crate) fn run(
        engine: &'a SyncEngine,
        state: &'a mut State,
        resolver: &'a dyn ConflictResolver,
    ) -> Result<SyncReport, SyncError> {
        let mut pass = Pass {
            engine,
            state,
            resolver,
            report: SyncReport::default(),
            again: false,
            offered: BTreeSet::new(),
        };
        for _ in 0..MAX_ROUNDS {
            pass.again = false;
            pass.round()?;
            if !pass.again {
                break;
            }
        }
        Ok(pass.report)
    }

    fn round(&mut self) -> Result<(), SyncError> {
        let (view, cursor) = self.pull_changes()?;

        let mut names: BTreeSet<BlobName> = self.engine.local.list()?.into_iter().collect();
        names.extend(view.seen.keys().cloned());
        names.extend(self.state.bases.keys().cloned());
        names.extend(self.state.parked.keys().cloned());
        let mut names: Vec<_> = names.into_iter().collect();
        // The key first (D5): if the two sides disagree about it, the pass must
        // stop before it has changed anything else.
        names.sort_by_key(|name| name.as_str() != KEY_BLOB);

        for name in &names {
            let seen = self.engine.dirty_count(name);
            let outcome = self.sync_name(name, &view)?;
            self.engine.settle(name, seen, outcome);
        }

        // Last, so a failure anywhere above re-reads the same changes.
        self.state.cursor = Some(cursor);
        self.state.save()?;
        Ok(())
    }

    /// Everything the remote reports since the last cursor, all pages.
    ///
    /// A cursor the remote has forgotten is not a failure: it is what a first
    /// run looks like, so the pass lists from scratch and compares against the
    /// bases it already has. A blob missing from a full listing was deleted
    /// there, which is how deletions made while the cursor was lost are found.
    fn pull_changes(&self) -> Result<(RemoteView, Cursor), SyncError> {
        match self.read_feed(self.state.cursor.as_ref()) {
            Err(SyncError::Remote(RemoteError::CursorExpired)) => {
                log::info!("the remote forgot the sync cursor; listing everything again");
                self.read_feed(None)
            }
            read => read,
        }
    }

    /// The feed from `since` to its end. It restarts from nothing, and does not
    /// resume, if a cursor expires part-way.
    fn read_feed(&self, since: Option<&Cursor>) -> Result<(RemoteView, Cursor), SyncError> {
        let mut view = RemoteView {
            complete: since.is_none(),
            seen: BTreeMap::new(),
        };
        let mut page = self.engine.remote.changes(since)?;
        loop {
            for change in page.changes {
                match change {
                    Change::Upserted {
                        name,
                        rev,
                        content_hash,
                    } => {
                        let hash = content_hash;
                        view.seen.insert(name, Seen::Present { rev, hash });
                    }
                    Change::Deleted { name } => {
                        view.seen.insert(name, Seen::Gone);
                    }
                }
            }
            if !page.has_more {
                return Ok((view, page.cursor));
            }
            page = self.engine.remote.changes(Some(&page.cursor))?;
        }
    }

    fn read_local(&self, name: &BlobName) -> Result<Option<Local>, SyncError> {
        Ok(self.engine.local.get(name)?.map(|bytes| Local {
            digest: Digest::of(&bytes),
            bytes,
        }))
    }

    fn sync_name(&mut self, name: &BlobName, view: &RemoteView) -> Result<Outcome, SyncError> {
        // An immutable blob that is here, has a base and is unchanged there is
        // synced by definition: skip reading and hashing it. Anything else
        // (no base, missing locally, changed remotely, parked) takes the
        // normal path below.
        if self.engine.is_immutable(name)
            && name.as_str() != KEY_BLOB
            && !self.state.parked.contains_key(name)
        {
            let base = self.state.bases.get(name);
            if base.is_some()
                && matches!(view.theirs(name, base), Theirs::Same)
                && self.engine.local.contains(name)?
            {
                return Ok(Outcome::Synced);
            }
        }
        let local = self.read_local(name)?;

        // Its own table: the key is never merged, never deleted and never
        // parked, so none of the arms below apply to it.
        if name.as_str() == KEY_BLOB {
            return self.sync_key(name, local, view);
        }

        if self.state.parked.contains_key(name) {
            return self.resume_parked(name, local, view);
        }

        let base = self.state.bases.get(name).cloned();
        match (local, view.theirs(name, base.as_ref())) {
            // Only we changed it, or nothing changed.
            (None, Theirs::Same) => match base {
                Some(base) => self.delete_remote(name, &base),
                None => Ok(Outcome::Synced),
            },
            (Some(local), Theirs::Same) => match base {
                Some(base) if base.hash == local.digest => Ok(Outcome::Synced),
                base => self.push(name, &local, base.as_ref()),
            },

            // Only they changed it.
            (None, Theirs::Edited { .. }) => self.pull(name),
            (Some(local), Theirs::Edited { rev, hash }) => match base {
                Some(base) if base.hash == local.digest => self.pull(name),
                _ => self.compare(name, &local, rev, hash),
            },
            (None, Theirs::Removed) => {
                self.state.forget(name)?;
                Ok(Outcome::Synced)
            }
            (Some(local), Theirs::Removed) => match base {
                Some(base) if base.hash == local.digest => self.delete_local(name),
                // Edited here, deleted there: keep the edit (D4). A journal
                // must not lose text.
                _ => self.push(name, &local, None),
            },
        }
    }

    /// What to do with the key blob, which is not an ordinary one (D5).
    ///
    /// It is never merged, and a pass that disagrees about it must change
    /// nothing: the key is the one thing every entry is sealed to, so an upload
    /// of a replaced key, or a deletion, would make the whole journal
    /// unreadable on every device. The rules that differ from `sync_name`'s
    /// table, all in one place so it can be read against it:
    ///
    /// - a local key that is not the one last synced is a mismatch, and stops
    ///   the pass (`conflict` enforces the same when the remote moved, but is
    ///   only reached then);
    /// - a remote edit is compared, never pulled over the local key;
    /// - a remote deletion is undone, not followed — without the key the
    ///   remote journal cannot be read, and neither can this one;
    /// - a local deletion is undone, not pushed — the remote copy is what
    ///   brings the key back.
    ///
    /// Only the key has a table of its own, so the arms it shares with the main
    /// one (nothing changed; created here, or arrived there) are spelled out
    /// rather than delegated.
    fn sync_key(
        &mut self,
        name: &BlobName,
        local: Option<Local>,
        view: &RemoteView,
    ) -> Result<Outcome, SyncError> {
        let base = self.state.bases.get(name).cloned();
        match (local, view.theirs(name, base.as_ref())) {
            (Some(local), _) if base.as_ref().is_some_and(|b| b.hash != local.digest) => {
                Err(SyncError::KeyMismatch)
            }
            (Some(_), Theirs::Same) if base.is_some() => Ok(Outcome::Synced),
            (Some(local), Theirs::Same | Theirs::Removed) => self.push(name, &local, None),
            // Identical bytes record a base; different ones reach `conflict`,
            // which returns KeyMismatch.
            (Some(local), Theirs::Edited { rev, hash }) => self.compare(name, &local, rev, hash),
            (None, Theirs::Same) if base.is_none() => Ok(Outcome::Synced),
            (None, Theirs::Same | Theirs::Edited { .. }) => self.restore_key(name),
            (None, Theirs::Removed) => {
                self.state.forget(name)?;
                Ok(Outcome::Synced)
            }
        }
    }

    /// Brings the remote's key here, where there is none.
    ///
    /// Created rather than written over: a key can appear locally between the
    /// read that found none and this write (first-run creation seals for about
    /// a second), and replacing it would leave the entries it has sealed
    /// unreadable. If one did, the next pass compares it with the base and
    /// stops on a mismatch.
    fn restore_key(&mut self, name: &BlobName) -> Result<Outcome, SyncError> {
        let Some((bytes, rev)) = self.engine.remote.get(name)? else {
            return Ok(Outcome::Skipped);
        };
        match self.engine.local.put_new(name, &bytes) {
            Ok(()) => self.record_pulled(name, &bytes, rev),
            Err(StoreError::AlreadyExists(_)) => Ok(Outcome::Skipped),
            Err(e) => Err(e.into()),
        }
    }

    /// Uploads `local`, conditioned on the remote still being where the base
    /// says (or, with no base, on there being nothing there).
    fn push(
        &mut self,
        name: &BlobName,
        local: &Local,
        base: Option<&Base>,
    ) -> Result<Outcome, SyncError> {
        let mut pre = match base {
            Some(base) => Precondition::Matches(base.rev.clone()),
            None => Precondition::Absent,
        };
        // Twice at most: a conflict whose other side has since been deleted is
        // retried once, as a create.
        for _ in 0..2 {
            match self.engine.remote.put(name, &local.bytes, pre.clone()) {
                Ok(rev) => {
                    // The hash of what was uploaded, not of whatever the local
                    // blob is by now: a save that landed meanwhile then reads as
                    // a local change next time, instead of being recorded as
                    // synced.
                    let base = Base {
                        rev,
                        hash: local.digest,
                    };
                    self.state.record(name, base)?;
                    self.report.pushed += 1;
                    return Ok(Outcome::Synced);
                }
                Err(RemoteError::Conflict { theirs }) => {
                    let Some(current) = self.engine.remote.get(name)? else {
                        pre = Precondition::Absent;
                        continue;
                    };
                    // If the write landed (Drive), `current` is our own bytes
                    // and `theirs` is what they overwrote. Otherwise the two are
                    // the same, and `conflict` is not given them twice.
                    let overwritten = theirs.map(|(bytes, _)| bytes);
                    return self.conflict(name, local, current, overwritten);
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(Outcome::Pending)
    }

    /// Downloads the remote's version over the local one.
    ///
    /// Only reached when the local copy was unchanged (or absent) when it was
    /// read. A save that lands in the instant before the write below is
    /// overwritten: the store has no compare-and-swap to prevent it.
    fn pull(&mut self, name: &BlobName) -> Result<Outcome, SyncError> {
        // Deleted since the feed said otherwise: the deletion is in the next
        // feed, and this blob is left as it is until then.
        let Some((bytes, rev)) = self.engine.remote.get(name)? else {
            return Ok(Outcome::Skipped);
        };
        self.engine.local.put(name, &bytes)?;
        self.record_pulled(name, &bytes, rev)
    }

    /// `bytes` are in the local store: the remote's `rev` is what they match.
    fn record_pulled(
        &mut self,
        name: &BlobName,
        bytes: &[u8],
        rev: Revision,
    ) -> Result<Outcome, SyncError> {
        let base = Base {
            rev,
            hash: Digest::of(bytes),
        };
        self.state.record(name, base)?;
        self.report.pulled += 1;
        Ok(Outcome::Synced)
    }

    fn delete_local(&mut self, name: &BlobName) -> Result<Outcome, SyncError> {
        self.engine.local.delete(name)?;
        self.state.forget(name)?;
        self.report.deleted += 1;
        Ok(Outcome::Synced)
    }

    /// Deletes the remote copy of a blob that was deleted here.
    fn delete_remote(&mut self, name: &BlobName, base: &Base) -> Result<Outcome, SyncError> {
        match self
            .engine
            .remote
            .delete(name, Precondition::Matches(base.rev.clone()))
        {
            Ok(()) => {
                self.state.forget(name)?;
                self.report.pushed += 1;
                Ok(Outcome::Synced)
            }
            // Edited there since we last synced. Deleting here does not outrank
            // an edit there: keep it (D4).
            Err(RemoteError::Conflict { theirs: None }) => match self.pull(name)? {
                // Already gone by now, which is what we wanted.
                Outcome::Skipped => {
                    self.state.forget(name)?;
                    Ok(Outcome::Synced)
                }
                outcome => Ok(outcome),
            },
            // The delete landed before the check (Drive), so the remote no
            // longer has their edit — the error does. Bring it back here, and
            // upload it again next round as a new blob.
            Err(RemoteError::Conflict {
                theirs: Some((bytes, _)),
            }) => {
                self.engine.local.put(name, &bytes)?;
                self.state.forget(name)?;
                self.report.pulled += 1;
                self.again = true;
                Ok(Outcome::Pending)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Both sides changed a blob the remote lists. If they changed it to the
    /// same thing, that is not a conflict.
    fn compare(
        &mut self,
        name: &BlobName,
        local: &Local,
        rev: Revision,
        hash: Option<String>,
    ) -> Result<Outcome, SyncError> {
        // The cheap way: the provider's hash, if it has one, without a
        // download.
        if let (Some(theirs), Some(ours)) = (hash, self.engine.remote.content_hash(&local.bytes)) {
            if theirs == ours {
                let base = Base {
                    rev,
                    hash: local.digest,
                };
                self.state.record(name, base)?;
                return Ok(Outcome::Synced);
            }
        }
        // Deleted since the feed said so: the edit wins (D4).
        let Some(current) = self.engine.remote.get(name)? else {
            return self.push(name, local, None);
        };
        self.conflict(name, local, current, None)
    }

    /// `ours` and the remote disagree about `name`. `current` is what the
    /// remote holds now. `overwritten` is `Some` only on a backend that wrote
    /// before it checked (Drive), where the other device's version is no
    /// longer what a `get` returns (see [`Parked`]); `None` means `current`
    /// *is* the other device's version.
    fn conflict(
        &mut self,
        name: &BlobName,
        ours: &Local,
        current: (Vec<u8>, Revision),
        overwritten: Option<Vec<u8>>,
    ) -> Result<Outcome, SyncError> {
        let (current_bytes, rev) = current;
        let remote_hash = Digest::of(&current_bytes);
        let (theirs, theirs_hash) = match overwritten {
            Some(bytes) => {
                let hash = Digest::of(&bytes);
                (bytes, hash)
            }
            None => (current_bytes, remote_hash),
        };

        if theirs_hash == ours.digest {
            // Both sides made the same change: nothing to resolve. The remote
            // may still hold something else (the overwrite case), in which case
            // the local bytes are a change to push.
            self.state.record(
                name,
                Base {
                    rev,
                    hash: remote_hash,
                },
            )?;
            return Ok(if remote_hash == ours.digest {
                Outcome::Synced
            } else {
                self.again = true;
                Outcome::Pending
            });
        }
        if name.as_str() == KEY_BLOB {
            return Err(SyncError::KeyMismatch);
        }

        self.state.park(
            name,
            Parked {
                rev,
                remote_hash,
                theirs,
            },
        )?;
        self.engine.set_conflicted(name, true);
        self.offer(name, &ours.bytes)
    }

    /// Hands a parked conflict to the resolver.
    fn offer(&mut self, name: &BlobName, ours: &[u8]) -> Result<Outcome, SyncError> {
        self.offered.insert(name.clone());
        self.report.conflicts += 1;
        let resolution = self
            .resolver
            .resolve(name, ours, &self.state.parked[name].theirs);
        if resolution == Resolution::Resolved {
            self.finish_resolution(name)?;
        }
        // Either way there is still something to upload: the merged result, or
        // the local change that could not be reconciled yet.
        Ok(Outcome::Pending)
    }

    /// The resolver's result is in the local store. Record the remote as it was
    /// when the conflict was parked, so the result is pushed against that, and
    /// leave the push to the next round.
    fn finish_resolution(&mut self, name: &BlobName) -> Result<(), SyncError> {
        let parked = &self.state.parked[name];
        let base = Base {
            rev: parked.rev.clone(),
            hash: parked.remote_hash,
        };
        self.state.unpark(name, Some(base))?;
        self.engine.set_conflicted(name, false);
        self.again = true;
        Ok(())
    }

    /// A conflict parked by an earlier pass: try the resolver again, having
    /// first checked that the remote has not moved on since.
    fn resume_parked(
        &mut self,
        name: &BlobName,
        local: Option<Local>,
        view: &RemoteView,
    ) -> Result<Outcome, SyncError> {
        // Deferred earlier in this very pass: asking again would get the same
        // answer.
        if self.offered.contains(name) {
            return Ok(Outcome::Pending);
        }

        let Some(local) = local else {
            // Deleted here while the conflict was parked. The other side's
            // edit stands (D4): restore it, and let the push against the
            // recorded remote state follow.
            let theirs = self.state.parked[name].theirs.clone();
            self.engine.local.put(name, &theirs)?;
            self.report.pulled += 1;
            self.finish_resolution(name)?;
            return Ok(Outcome::Pending);
        };

        let parked_rev = self.state.parked[name].rev.clone();
        let moved = match view.seen(name) {
            Some(Seen::Present { rev, .. }) => *rev != parked_rev,
            Some(Seen::Gone) => true,
            None => false,
        };
        if moved {
            // The remote changed again since we parked it. Look at what is
            // there now; if it is gone, the edit wins (D4).
            let Some((bytes, rev)) = self.engine.remote.get(name)? else {
                self.state.unpark(name, None)?;
                self.engine.set_conflicted(name, false);
                return self.push(name, &local, None);
            };
            let parked = Parked {
                rev,
                remote_hash: Digest::of(&bytes),
                theirs: bytes,
            };
            self.state.park(name, parked)?;
        }

        if Digest::of(&self.state.parked[name].theirs) == local.digest {
            // The local copy has since become the other side's version.
            self.finish_resolution(name)?;
            return Ok(Outcome::Pending);
        }
        self.offer(name, &local.bytes)
    }
}
