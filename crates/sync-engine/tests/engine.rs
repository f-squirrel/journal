//! What a pass does with each blob, run between two simulated devices.

mod common;

use std::sync::Arc;

use common::*;
use journal_remote::{ConflictMode, MemoryRemote, RemoteError, RemoteStore};
use journal_sync_engine::{SyncEngine, SyncError, SyncErrorKind, SyncReport, KEY_BLOB};

const BOTH_MODES: [ConflictMode; 2] = [ConflictMode::Atomic, ConflictMode::DetectAfterWrite];

fn remote_in(mode: ConflictMode) -> Arc<MemoryRemote> {
    Arc::new(MemoryRemote::new().with_conflict_mode(mode))
}

/// Two devices that have both synced `blobs`.
fn synced_pair(remote: &Arc<MemoryRemote>, blobs: &[(&str, &[u8])]) -> (Device, Device) {
    let (a, b) = (Device::new(remote.clone()), Device::new(remote.clone()));
    for (name, bytes) in blobs {
        a.put(name, bytes);
    }
    a.sync();
    b.sync();
    (a, b)
}

#[test]
fn a_blob_created_on_one_device_appears_on_the_other() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = (Device::new(remote.clone()), Device::new(remote.clone()));

    a.put("x.json", b"one");
    assert_eq!(
        a.sync(),
        SyncReport {
            pushed: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        b.sync(),
        SyncReport {
            pulled: 1,
            ..Default::default()
        }
    );
    assert_eq!(b.read("x.json").unwrap(), b"one");

    // Nothing left to do on either side, not even to see its own change echoed
    // back by the feed.
    assert_eq!(a.sync(), SyncReport::default());
    assert_eq!(b.sync(), SyncReport::default());
}

#[test]
fn an_edit_on_the_other_device_comes_back() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = synced_pair(&remote, &[("x.json", b"one")]);

    b.put("x.json", b"two");
    assert_eq!(b.sync().pushed, 1);
    assert_eq!(a.sync().pulled, 1);
    assert_eq!(a.read("x.json").unwrap(), b"two");
    assert_eq!(a.sync(), SyncReport::default());
}

#[test]
fn a_deletion_reaches_the_other_device_and_is_not_undone() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = synced_pair(&remote, &[("x.json", b"one"), ("y.json", b"keep")]);

    a.delete("x.json");
    assert_eq!(a.sync().pushed, 1);
    assert_eq!(
        b.sync(),
        SyncReport {
            deleted: 1,
            ..Default::default()
        }
    );
    assert_eq!(b.read("x.json"), None);

    // Neither side brings it back on later passes.
    assert_eq!(b.sync(), SyncReport::default());
    assert_eq!(a.sync(), SyncReport::default());
    assert_eq!(
        remote_snapshot(&*remote),
        snapshot_of(&[("y.json", b"keep")])
    );
    assert_eq!(b.snapshot(), snapshot_of(&[("y.json", b"keep")]));
}

/// The two ways two edits meet: the pass sees the other side's change in the
/// feed, or the other side gets in between the feed and the write.
enum Meeting {
    SeenInFeed,
    RaceOnPush,
}

fn edit_edit(mode: ConflictMode, meeting: Meeting, a_edit: &[u8], b_edit: &[u8]) {
    let base = MemoryRemote::new().with_conflict_mode(mode);
    let remote = Arc::new(base);
    let hooked = Hooked::new(remote.clone());
    let a = Arc::new(Device::new(remote.clone()));
    let b = Device::new(hooked.clone());
    a.put("x.json", b"v0");
    a.sync();
    b.sync();

    a.put("x.json", a_edit);
    b.put("x.json", b_edit);
    match meeting {
        Meeting::SeenInFeed => {
            a.sync();
        }
        Meeting::RaceOnPush => {
            let a = a.clone();
            hooked.before_next_put(move || {
                a.sync();
            });
        }
    }

    let resolver = KeepBoth::new(&b);
    assert_eq!(b.sync_with(&resolver).conflicts, 1, "{mode:?}");
    assert_eq!(
        resolver.calls(),
        [("x.json".to_string(), b_edit.to_vec(), a_edit.to_vec())],
        "{mode:?}"
    );

    // Both devices end up with both versions: the winner under the original
    // name, the other as a copy.
    a.sync();
    b.sync();
    a.sync();
    let (winner, loser) = if b_edit >= a_edit {
        (b_edit, a_edit)
    } else {
        (a_edit, b_edit)
    };
    let expected = snapshot_of(&[("x.json", winner), ("x.json.copy", loser)]);
    assert_eq!(a.snapshot(), expected, "{mode:?}");
    assert_eq!(b.snapshot(), expected, "{mode:?}");
    assert_eq!(remote_snapshot(&*remote), expected, "{mode:?}");
    for device in [&*a, &b] {
        let status = device.status();
        assert!(status.pending.is_empty() && status.conflicted.is_empty());
    }
    // And it stays settled.
    assert_eq!(a.sync(), SyncReport::default());
    assert_eq!(b.sync(), SyncReport::default());
}

#[test]
fn conflicting_edits_reach_the_resolver_and_both_survive() {
    for mode in BOTH_MODES {
        // Once with each side winning the merge.
        for (a_edit, b_edit) in [(&b"a-edit"[..], &b"z-edit"[..]), (b"z-edit", b"a-edit")] {
            edit_edit(mode, Meeting::SeenInFeed, a_edit, b_edit);
            edit_edit(mode, Meeting::RaceOnPush, a_edit, b_edit);
        }
    }
}

#[test]
fn both_devices_making_the_same_edit_is_not_a_conflict() {
    for mode in BOTH_MODES {
        let remote = remote_in(mode);
        let (a, b) = synced_pair(&remote, &[("x.json", b"v0")]);
        a.put("x.json", b"same");
        b.put("x.json", b"same");
        a.sync();
        // `NoConflicts` panics if the resolver is asked.
        b.sync();
        assert_eq!(b.status().pending.len(), 0, "{mode:?}");
        assert_eq!(b.sync(), SyncReport::default());
    }
}

#[test]
fn an_edit_beats_a_delete_whichever_reaches_the_remote_first() {
    for mode in BOTH_MODES {
        // The delete is pushed first, and the edit finds the blob gone.
        let remote = remote_in(mode);
        let (a, b) = synced_pair(&remote, &[("x.json", b"v0")]);
        a.delete("x.json");
        b.put("x.json", b"edit");
        a.sync();
        b.sync();
        a.sync();
        assert_eq!(a.read("x.json").unwrap(), b"edit", "{mode:?}");
        assert_eq!(remote_snapshot(&*remote), b.snapshot(), "{mode:?}");
        assert_eq!(a.snapshot(), b.snapshot(), "{mode:?}");

        // The edit is pushed first, and the delete finds it.
        let remote = remote_in(mode);
        let (a, b) = synced_pair(&remote, &[("x.json", b"v0")]);
        a.delete("x.json");
        b.put("x.json", b"edit");
        b.sync();
        a.sync();
        b.sync();
        assert_eq!(a.read("x.json").unwrap(), b"edit", "{mode:?}");
        assert_eq!(a.snapshot(), b.snapshot(), "{mode:?}");
    }
}

#[test]
fn an_edit_that_lands_between_a_pass_reading_and_deleting_is_kept() {
    for mode in BOTH_MODES {
        let remote = remote_in(mode);
        let hooked = Hooked::new(remote.clone());
        let a = Arc::new(Device::new(remote.clone()));
        let b = Device::new(hooked.clone());
        a.put("x.json", b"v0");
        a.sync();
        b.sync();

        b.delete("x.json");
        let a2 = a.clone();
        hooked.before_next_delete(move || {
            a2.put("x.json", b"edit");
            a2.sync();
        });
        b.sync();
        a.sync();
        b.sync();

        assert_eq!(b.read("x.json").unwrap(), b"edit", "{mode:?}");
        assert_eq!(a.snapshot(), b.snapshot(), "{mode:?}");
        assert_eq!(remote_snapshot(&*remote), b.snapshot(), "{mode:?}");
    }
}

#[test]
fn a_device_that_was_offline_catches_up_after_many_changes() {
    // Small pages, so the catch-up spans several.
    let remote = Arc::new(MemoryRemote::new().with_page_size(3));
    let (a, b) = synced_pair(
        &remote,
        &[("a.json", b"1"), ("b.json", b"1"), ("c.json", b"1")],
    );

    // B is away while A makes a run of changes, syncing along the way.
    for round in 0..5 {
        a.put(&format!("new{round}.json"), b"n");
        a.put("a.json", format!("edit{round}").as_bytes());
        if round == 2 {
            a.delete("b.json");
        }
        a.sync();
    }
    a.delete("new0.json");
    a.sync();

    b.sync();
    assert_eq!(b.snapshot(), a.snapshot());
    assert_eq!(b.read("a.json").unwrap(), b"edit4");
    assert_eq!(b.read("b.json"), None);
    assert_eq!(b.read("new0.json"), None);
    assert_eq!(b.sync(), SyncReport::default());
}

#[test]
fn a_new_device_joins_a_populated_remote() {
    let remote = Arc::new(MemoryRemote::new().with_page_size(2));
    let a = Device::new(remote.clone());
    a.put(KEY_BLOB, b"sealed key");
    for i in 0..5 {
        a.put(&format!("{i}.json"), b"entry");
    }
    a.sync();

    let c = Device::new(remote.clone());
    let report = c.sync();
    assert_eq!(report.pulled, 6);
    assert_eq!(c.snapshot(), a.snapshot());
    // Nothing on it was written locally, so nothing is waiting to go back.
    assert!(c.status().pending.is_empty());
    assert_eq!(c.sync(), SyncReport::default());
}

#[test]
fn a_device_that_already_holds_identical_content_joins_without_a_conflict() {
    // With and without a provider hash, so both ways of comparing are covered:
    // the hash from the feed, and a download.
    for remote in [
        Arc::new(MemoryRemote::new()),
        Arc::new(MemoryRemote::new().without_content_hash()),
    ] {
        let a = Device::new(remote.clone());
        a.put("x.json", b"same");
        a.sync();

        let c = Device::new(remote.clone());
        c.put_quietly("x.json", b"same");
        c.reopen();
        assert_eq!(c.sync(), SyncReport::default());
        assert!(c.status().pending.is_empty());

        // And it is now tracked as synced: an edit is pushed, not re-compared.
        c.put("x.json", b"edited");
        assert_eq!(c.sync().pushed, 1);
        assert_eq!(a.sync().pulled, 1);
        assert_eq!(a.read("x.json").unwrap(), b"edited");
    }
}

#[test]
fn a_device_with_different_content_for_a_new_name_hits_the_resolver() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    a.put("x.json", b"from a");
    a.sync();

    let c = Device::new(remote.clone());
    c.put("x.json", b"from c");
    let resolver = KeepBoth::new(&c);
    assert_eq!(c.sync_with(&resolver).conflicts, 1);
    assert_eq!(
        resolver.calls(),
        [("x.json".into(), b"from c".to_vec(), b"from a".to_vec())]
    );
}

#[test]
fn differing_keys_stop_the_pass_before_anything_changes() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    a.put(KEY_BLOB, b"key from a");
    a.put("a.json", b"entry a");
    a.sync();

    // B made its own journal before ever connecting.
    let b = Device::new(remote.clone());
    b.put(KEY_BLOB, b"key from b");
    b.put("b.json", b"entry b");
    b.put("a.json", b"a different a");
    // Sorts before the key, so only the engine's own ordering keeps it from
    // being uploaded ahead of the mismatch being found.
    b.put("-first.json", b"entry -");
    let local_before = b.snapshot();
    let remote_before = remote_snapshot(&*remote);

    let err = b.try_sync().unwrap_err();
    assert!(matches!(err, SyncError::KeyMismatch), "{err:?}");
    assert_eq!(err.kind(), SyncErrorKind::KeyMismatch);
    assert_eq!(b.status().last_error, Some(SyncErrorKind::KeyMismatch));

    // Nothing was pulled, pushed, parked or recorded.
    assert_eq!(b.snapshot(), local_before);
    assert_eq!(remote_snapshot(&*remote), remote_before);
    assert_eq!(b.state_bytes(), None);
    assert!(b.status().conflicted.is_empty());

    // It keeps refusing until the user decides.
    assert!(matches!(b.try_sync(), Err(SyncError::KeyMismatch)));
}

#[test]
fn the_same_key_on_both_sides_is_not_a_mismatch() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    a.put(KEY_BLOB, b"the key");
    a.sync();

    let b = Device::new(remote.clone());
    b.put_quietly(KEY_BLOB, b"the key");
    b.reopen();
    assert_eq!(b.sync(), SyncReport::default());
}

/// A device that has synced, with the key and one entry on both sides.
fn synced_with_key(remote: &Arc<MemoryRemote>) -> Device {
    let a = Device::new(remote.clone());
    a.put(KEY_BLOB, b"the key");
    a.put("a.json", b"entry a");
    a.sync();
    a
}

#[test]
fn a_replaced_key_is_not_uploaded() {
    let remote = Arc::new(MemoryRemote::new());
    let a = synced_with_key(&remote);
    let remote_before = remote_snapshot(&*remote);

    // The remote has not moved, so the ordinary "only we changed it" arm would
    // push this under the base's revision, and the remote would accept it.
    a.put(KEY_BLOB, b"a brand new key");
    a.put("b.json", b"entry b");

    let err = a.try_sync().unwrap_err();
    assert!(matches!(err, SyncError::KeyMismatch), "{err:?}");
    assert_eq!(remote_snapshot(&*remote), remote_before);
    assert_eq!(a.read(KEY_BLOB).unwrap(), b"a brand new key");

    // It keeps refusing until the user decides.
    assert!(matches!(a.try_sync(), Err(SyncError::KeyMismatch)));
    assert_eq!(remote_snapshot(&*remote), remote_before);
}

#[test]
fn a_locally_deleted_key_is_restored_not_deleted_remotely() {
    use journal_store::BlobStore;

    let remote = Arc::new(MemoryRemote::new());
    let a = synced_with_key(&remote);
    let remote_before = remote_snapshot(&*remote);

    // As when the entries directory is wiped while `sync-state.json`, which
    // lives beside it and not in it, survives.
    a.store.delete(&n(KEY_BLOB)).unwrap();
    a.reopen();

    // The remote's copy is what brings the key back: without it the entries
    // that survived (or arrive) cannot be opened.
    let report = a.sync();
    assert_eq!(report.pulled, 1);
    assert_eq!(a.read(KEY_BLOB).unwrap(), b"the key");
    assert_eq!(remote_snapshot(&*remote), remote_before);
    assert_eq!(a.read("a.json").unwrap(), b"entry a");

    // And it is an ordinary synced key afterwards.
    assert_eq!(a.sync(), SyncReport::default());
    assert_eq!(remote_snapshot(&*remote), remote_before);
}

mod racy {
    use journal_fs_store::FsStore;
    use journal_store::{BlobName, BlobStore, StoreError};

    /// An [`FsStore`] that finds a key already there whenever it is asked to
    /// create one, as if first-run creation had written its own since the pass
    /// looked.
    pub struct KeyAppearsFirst(pub FsStore);

    impl BlobStore for KeyAppearsFirst {
        fn get(&self, name: &BlobName) -> Result<Option<Vec<u8>>, StoreError> {
            self.0.get(name)
        }
        fn put(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
            self.0.put(name, bytes)
        }
        fn put_new(&self, name: &BlobName, _bytes: &[u8]) -> Result<(), StoreError> {
            self.0.put(name, b"a key created here")?;
            Err(StoreError::AlreadyExists(name.as_str().to_owned()))
        }
        fn delete(&self, name: &BlobName) -> Result<(), StoreError> {
            self.0.delete(name)
        }
        fn list(&self) -> Result<Vec<BlobName>, StoreError> {
            self.0.list()
        }
        fn contains(&self, name: &BlobName) -> Result<bool, StoreError> {
            self.0.contains(name)
        }
    }
}

#[test]
fn a_restored_key_does_not_replace_one_created_meanwhile() {
    use journal_store::BlobStore;

    let remote = Arc::new(MemoryRemote::new());
    let a = synced_with_key(&remote);
    let remote_before = remote_snapshot(&*remote);

    a.store.delete(&n(KEY_BLOB)).unwrap();
    let store = racy::KeyAppearsFirst(a.store.clone());
    let engine = SyncEngine::open(
        Arc::new(store),
        remote.clone(),
        a.state_path(),
        Box::new(|_| false),
        Box::new(|_| {}),
    )
    .unwrap();

    // The pass leaves the key that is there alone, and does not upload it.
    engine.sync_once(&NoConflicts).unwrap();
    assert_eq!(a.read(KEY_BLOB).unwrap(), b"a key created here");
    assert_eq!(remote_snapshot(&*remote), remote_before);

    // Which the next pass sees is not the key it last synced.
    let err = engine.sync_once(&NoConflicts).unwrap_err();
    assert!(matches!(err, SyncError::KeyMismatch), "{err:?}");
    assert_eq!(remote_snapshot(&*remote), remote_before);
}

#[test]
fn a_remotely_deleted_key_is_restored_not_followed() {
    use journal_remote::Precondition;

    let remote = Arc::new(MemoryRemote::new());
    let a = synced_with_key(&remote);

    // `Precondition` has no unconditional delete: match the revision it has.
    let (_, rev) = remote.get(&n(KEY_BLOB)).unwrap().unwrap();
    remote
        .delete(&n(KEY_BLOB), Precondition::Matches(rev))
        .unwrap();
    assert_eq!(remote.get(&n(KEY_BLOB)).unwrap(), None);

    // Following the deletion would leave the remote journal with no key at all.
    a.sync();
    assert_eq!(a.read(KEY_BLOB).unwrap(), b"the key");
    assert_eq!(remote.get(&n(KEY_BLOB)).unwrap().unwrap().0, b"the key");
    assert_eq!(a.read("a.json").unwrap(), b"entry a");
    assert_eq!(remote_snapshot(&*remote).len(), 2);
}

#[test]
fn a_transient_failure_leaves_everything_as_it_was() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = synced_pair(&remote, &[("x.json", b"v0")]);
    b.put("x.json", b"edit");
    b.put("y.json", b"new");
    a.put("z.json", b"other");
    a.sync();

    let state = b.state_bytes();
    let local = b.snapshot();
    let remote_before = remote_snapshot(&*remote);

    remote.fail_next(RemoteError::Transient("offline".into()));
    let err = b.try_sync().unwrap_err();
    assert_eq!(err.kind(), SyncErrorKind::Offline);
    assert_eq!(b.state_bytes(), state);
    assert_eq!(b.snapshot(), local);
    assert_eq!(remote_snapshot(&*remote), remote_before);

    // And the retry does the whole job.
    let report = b.sync();
    assert_eq!((report.pushed, report.pulled), (2, 1));
}

#[test]
fn a_first_pass_that_fails_writes_no_state() {
    let remote = Arc::new(MemoryRemote::new());
    let a = Device::new(remote.clone());
    a.put("x.json", b"v0");
    remote.fail_next(RemoteError::Unauthorized);
    assert_eq!(
        a.try_sync().unwrap_err().kind(),
        SyncErrorKind::Unauthorized
    );
    assert_eq!(a.state_bytes(), None);
}

#[test]
fn a_failure_at_any_point_in_a_pass_is_recovered_by_running_again() {
    let mut failures = 0;
    // Break the pass at every call it makes, one place per iteration.
    for calls_before_failure in 0..14 {
        let remote = Arc::new(MemoryRemote::new());
        let (a, b) = synced_pair(
            &remote,
            &[("a.json", b"1"), ("b.json", b"1"), ("c.json", b"1")],
        );
        a.put("a.json", b"a edited");
        a.delete("b.json");
        a.put("d.json", b"a new");
        b.put("c.json", b"c edited");
        b.put("e.json", b"b new");

        remote.fail_after(
            calls_before_failure,
            RemoteError::Transient("connection reset".into()),
        );
        failures += a.try_sync().is_err() as usize;
        // As if the app quit mid-pass: nothing survives but the files.
        a.reopen();
        failures += b.try_sync().is_err() as usize;
        b.reopen();

        for _ in 0..3 {
            a.sync_until_ok();
            b.sync_until_ok();
        }
        let expected = snapshot_of(&[
            ("a.json", b"a edited"),
            ("c.json", b"c edited"),
            ("d.json", b"a new"),
            ("e.json", b"b new"),
        ]);
        assert_eq!(
            a.snapshot(),
            expected,
            "failing after {calls_before_failure}"
        );
        assert_eq!(
            b.snapshot(),
            expected,
            "failing after {calls_before_failure}"
        );
        assert_eq!(remote_snapshot(&*remote), expected);
        assert!(a.status().pending.is_empty() && b.status().pending.is_empty());
    }
    assert!(
        failures > 5,
        "the fault was injected too rarely to mean much"
    );
}

#[test]
fn a_corrupt_state_file_is_recovered_by_comparing_afresh() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = synced_pair(&remote, &[("x.json", b"one"), ("y.json", b"two")]);
    a.put("x.json", b"edit");
    a.sync();

    std::fs::write(b.state_path(), b"{ not json").unwrap();
    b.reopen();
    // With no base every blob is compared afresh. y matches. x does not, and b
    // cannot tell that it simply never saw a's edit, so it is offered as a
    // conflict rather than silently taking either side.
    let resolver = KeepBoth::new(&b);
    b.sync_with(&resolver);
    a.sync();
    b.sync();
    // Nothing was lost, and the devices agree.
    assert_eq!(a.snapshot(), b.snapshot());
    assert!(a.snapshot().values().any(|v| v == b"edit"));
}

#[test]
fn the_engine_reads_a_remote_deletion_out_of_a_full_listing() {
    // Cursor lost, base kept: a blob missing from the full listing is gone.
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = synced_pair(&remote, &[("x.json", b"one"), ("y.json", b"two")]);
    a.delete("x.json");
    a.sync();

    let state = String::from_utf8(b.state_bytes().unwrap()).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&state).unwrap();
    json["cursor"] = serde_json::Value::Null;
    std::fs::write(b.state_path(), json.to_string()).unwrap();
    b.reopen();

    assert_eq!(b.sync().deleted, 1);
    assert_eq!(b.snapshot(), snapshot_of(&[("y.json", b"two")]));
}

#[test]
fn the_key_blob_name_is_the_one_the_key_crate_writes() {
    use journal_key::Keyring;
    use journal_store::BlobStore;
    use secrecy::SecretString;

    let device = Device::new(Arc::new(MemoryRemote::new()));
    Keyring::create(&device.store, &SecretString::from("passphrase")).unwrap();
    let names: Vec<_> = device.store.list().unwrap();
    assert_eq!(names, [n(KEY_BLOB)]);
}

#[test]
fn a_cursor_the_remote_forgot_is_relisted_and_finds_what_happened_meanwhile() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = synced_pair(
        &remote,
        &[("x.json", b"1"), ("y.json", b"1"), ("z.json", b"1")],
    );
    a.put("x.json", b"a edited");
    a.delete("y.json");
    a.put("w.json", b"a new");
    a.sync();
    // B has edits of its own, one of them to what A deleted (D4: it survives).
    b.put("y.json", b"b edited too");
    b.put("z.json", b"b edited");

    remote.fail_next(RemoteError::CursorExpired);
    let report = b.sync();

    assert_eq!(
        report,
        SyncReport {
            pulled: 2,
            pushed: 2,
            ..Default::default()
        }
    );
    assert_eq!(
        b.snapshot(),
        snapshot_of(&[
            ("w.json", b"a new"),
            ("x.json", b"a edited"),
            ("y.json", b"b edited too"),
            ("z.json", b"b edited"),
        ])
    );
    a.sync();
    assert_eq!(a.snapshot(), b.snapshot());
    assert_eq!(remote_snapshot(&*remote), b.snapshot());
}

#[test]
fn a_deletion_made_while_the_cursor_was_lost_is_found_by_absence() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = synced_pair(&remote, &[("x.json", b"1"), ("y.json", b"1")]);
    a.delete("x.json");
    a.sync();

    remote.fail_next(RemoteError::CursorExpired);

    assert_eq!(b.sync().deleted, 1);
    assert_eq!(b.snapshot(), snapshot_of(&[("y.json", b"1")]));
}

#[test]
fn a_cursor_that_expires_part_way_through_paging_starts_over() {
    let remote = Arc::new(MemoryRemote::new().with_page_size(1));
    let (a, b) = synced_pair(&remote, &[("x.json", b"1"), ("y.json", b"1")]);
    a.put("x.json", b"2");
    a.put("y.json", b"2");
    a.put("z.json", b"2");
    a.sync();

    // The first page reads, the second is refused.
    remote.fail_after(1, RemoteError::CursorExpired);
    b.sync();

    assert_eq!(b.snapshot(), a.snapshot());
    assert_eq!(b.snapshot().len(), 3);
}

#[test]
fn a_remote_that_refuses_even_a_fresh_listing_is_a_failure() {
    let remote = Arc::new(MemoryRemote::new());
    let (_, b) = synced_pair(&remote, &[("x.json", b"1")]);

    remote.fail_next(RemoteError::CursorExpired);
    remote.fail_next(RemoteError::CursorExpired);

    // Not retried forever: the pass gives up and says so.
    assert_eq!(b.try_sync().unwrap_err().kind(), SyncErrorKind::Other);
    // And the next one, with the remote sane again, is fine.
    b.sync();
}

#[test]
fn a_blob_deleted_on_both_devices_is_forgotten_without_fuss() {
    let remote = Arc::new(MemoryRemote::new());
    let (a, b) = synced_pair(&remote, &[("x.json", b"one"), ("y.json", b"keep")]);

    a.delete("x.json");
    a.sync();
    // B deleted it too before hearing about it.
    b.delete("x.json");
    assert_eq!(b.try_sync().unwrap().pushed, 0);
    assert_eq!(b.read("x.json"), None);
    assert_eq!(b.read("y.json").unwrap(), b"keep");
    assert_eq!(b.sync(), SyncReport::default());
    assert!(!remote_snapshot(&*remote).contains_key("x.json"));
}

#[test]
fn a_key_missing_on_both_sides_is_forgotten_not_invented() {
    use journal_remote::Precondition;

    let remote = Arc::new(MemoryRemote::new());
    let a = synced_with_key(&remote);

    let (_, rev) = remote.get(&n(KEY_BLOB)).unwrap().unwrap();
    remote
        .delete(&n(KEY_BLOB), Precondition::Matches(rev))
        .unwrap();
    a.delete(KEY_BLOB);

    a.sync();
    assert_eq!(a.read(KEY_BLOB), None);
    assert_eq!(remote.get(&n(KEY_BLOB)).unwrap(), None);
    assert_eq!(a.read("a.json").unwrap(), b"entry a");
}
