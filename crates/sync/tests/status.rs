//! What the engine says is not uploaded yet, and when it says it.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use common::*;
use journal_remote::{MemoryRemote, RemoteError};
use journal_store::{BlobName, BlobStore};
use journal_sync::SyncErrorKind;

fn names(list: &[&str]) -> BTreeSet<BlobName> {
    list.iter().map(|name| n(name)).collect()
}

#[test]
fn a_new_an_edited_and_a_deleted_blob_are_each_pending() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    for name in ["x.json", "y.json", "z.json"] {
        a.put(name, b"v0");
    }
    a.sync();
    assert!(a.status().pending.is_empty());

    // Changes the engine is not told about: found by the scan when it opens.
    a.put_quietly("new.json", b"n");
    a.put_quietly("x.json", b"edited");
    a.store.delete(&n("y.json")).unwrap();
    a.reopen();
    assert_eq!(a.status().pending, names(&["new.json", "x.json", "y.json"]));

    let report = a.sync();
    assert_eq!(report.pushed, 3);
    let status = a.status();
    assert!(status.pending.is_empty());
    assert!(status.last_success.is_some());
    assert_eq!(status.last_error, None);
    assert!(!status.running);
}

#[test]
fn pending_survives_a_restart_without_being_saved() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    a.put("x.json", b"v0");
    a.sync();

    a.put("x.json", b"edited");
    assert_eq!(a.status().pending, names(&["x.json"]));
    // Dropped without syncing.
    a.reopen();
    assert_eq!(a.status().pending, names(&["x.json"]));
}

#[test]
fn a_blob_is_pending_the_moment_it_is_marked_dirty() {
    let a = Device::new(Arc::new(MemoryRemote::new()));
    a.engine().mark_dirty(&n("q.json"));
    assert_eq!(a.status().pending, names(&["q.json"]));
    let events = a.events.lock().unwrap();
    assert_eq!(events.last().unwrap().pending, names(&["q.json"]));
}

#[test]
fn a_failed_pass_leaves_what_it_did_not_finish_pending() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    for name in ["a.json", "b.json", "c.json"] {
        a.put(name, b"new");
    }

    // The feed call and the first upload succeed; the second upload fails.
    remote.fail_after(2, RemoteError::Transient("offline".into()));
    assert!(a.try_sync().is_err());
    let status = a.status();
    assert_eq!(status.pending, names(&["b.json", "c.json"]));
    assert_eq!(status.last_error, Some(SyncErrorKind::Offline));
    assert_eq!(status.last_success, None);
    assert!(!status.running);

    a.sync();
    let status = a.status();
    assert!(status.pending.is_empty());
    assert_eq!(status.last_error, None, "cleared by the next success");
    assert!(status.last_success.is_some());
}

#[test]
fn each_failure_is_reported_as_the_kind_the_app_acts_on() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    for (error, kind) in [
        (
            RemoteError::RateLimited {
                retry_after: std::time::Duration::from_secs(5),
            },
            SyncErrorKind::RateLimited,
        ),
        (RemoteError::Unauthorized, SyncErrorKind::Unauthorized),
        (RemoteError::Other("odd".into()), SyncErrorKind::Other),
    ] {
        remote.fail_next(error);
        assert!(a.try_sync().is_err());
        assert_eq!(a.status().last_error, Some(kind));
    }
}

#[test]
fn a_save_that_lands_while_its_blob_is_uploading_stays_pending() {
    let remote = Arc::new(MemoryRemote::new());
    let hooked = Hooked::new(remote.clone());
    let a = Arc::new(Device::new(hooked.clone()));
    a.put("x.json", b"old");

    let during = a.clone();
    hooked.before_next_put(move || during.put("x.json", b"new"));
    a.sync();

    // The upload carried the old bytes, so the blob must not read as synced.
    assert_eq!(
        remote_snapshot(&*remote),
        snapshot_of(&[("x.json", b"old")])
    );
    assert_eq!(a.status().pending, names(&["x.json"]));

    // The next pass uploads what was saved meanwhile.
    a.sync();
    assert_eq!(
        remote_snapshot(&*remote),
        snapshot_of(&[("x.json", b"new")])
    );
    assert!(a.status().pending.is_empty());
}

/// A conflict made by two edits, with `b` left holding it as `resolver` decides.
fn parked_conflict(remote: &Arc<MemoryRemote>) -> (Device, Device) {
    let (a, b) = (Device::new(remote.clone()), Device::new(remote.clone()));
    a.put("x.json", b"v0");
    a.sync();
    b.sync();
    a.put("x.json", b"a-edit");
    b.put("x.json", b"b-edit");
    a.sync();
    (a, b)
}

#[test]
fn a_deferred_conflict_stays_conflicted_and_pending_until_resolved_and_pushed() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = parked_conflict(&remote);

    let deferring = KeepBoth::deferring(&b);
    assert_eq!(b.sync_with(&deferring).conflicts, 1);
    assert_eq!(deferring.calls().len(), 1, "asked once per pass");
    assert_eq!(b.status().conflicted, names(&["x.json"]));
    assert!(b.status().pending.contains(&n("x.json")));

    // The parked conflict is on disk, so a restart does not forget it.
    b.reopen();
    assert_eq!(b.status().conflicted, names(&["x.json"]));
    assert!(b.status().pending.contains(&n("x.json")));

    // And it is offered again on the next pass, still deferred.
    b.sync_with(&deferring);
    assert_eq!(deferring.calls().len(), 2);
    assert_eq!(b.status().conflicted, names(&["x.json"]));

    // Once the resolver can, one pass resolves it and uploads the result.
    let resolver = KeepBoth::new(&b);
    b.sync_with(&resolver);
    let status = b.status();
    assert!(status.conflicted.is_empty() && status.pending.is_empty());
    a.sync();
    assert_eq!(a.snapshot(), b.snapshot());
    assert_eq!(
        a.snapshot(),
        snapshot_of(&[("x.json", b"b-edit"), ("x.json.copy", b"a-edit")])
    );
}

#[test]
fn a_parked_conflict_is_offered_the_newer_version_if_the_remote_moved_on() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = parked_conflict(&remote);
    b.sync_with(&KeepBoth::deferring(&b));

    a.put("x.json", b"a-edit-2");
    a.sync();

    let resolver = KeepBoth::new(&b);
    b.sync_with(&resolver);
    assert_eq!(
        resolver.calls(),
        [("x.json".into(), b"b-edit".to_vec(), b"a-edit-2".to_vec())]
    );
    a.sync();
    assert_eq!(a.snapshot(), b.snapshot());
}

#[test]
fn deleting_an_entry_whose_conflict_is_parked_restores_the_other_sides_edit() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = parked_conflict(&remote);
    b.sync_with(&KeepBoth::deferring(&b));

    b.delete("x.json");
    let resolver = KeepBoth::new(&b);
    b.sync_with(&resolver);

    assert!(
        resolver.calls().is_empty(),
        "there is nothing left to merge"
    );
    assert_eq!(b.read("x.json").unwrap(), b"a-edit");
    assert!(b.status().conflicted.is_empty() && b.status().pending.is_empty());
    a.sync();
    assert_eq!(a.snapshot(), b.snapshot());
}

#[test]
fn every_change_to_the_status_is_reported() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    a.put("x.json", b"v0");
    a.events.lock().unwrap().clear();

    a.sync();
    {
        let events = a.events.lock().unwrap();
        let first_running = events.iter().position(|s| s.running).expect("start");
        let cleared = events
            .iter()
            .position(|s| s.pending.is_empty())
            .expect("pending cleared");
        let done = events.iter().rposition(|s| !s.running).expect("end");
        assert!(first_running < cleared && cleared < done, "{events:#?}");
        let last = events.last().unwrap();
        assert!(!last.running && last.last_success.is_some() && last.last_error.is_none());
    }

    a.put("y.json", b"new");
    remote.fail_next(RemoteError::Transient("offline".into()));
    a.events.lock().unwrap().clear();
    assert!(a.try_sync().is_err());
    let events = a.events.lock().unwrap();
    let last = events.last().unwrap();
    assert!(!last.running);
    assert_eq!(last.last_error, Some(SyncErrorKind::Offline));
    assert!(events.first().unwrap().running);
}
