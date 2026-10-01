//! `probe_key`: the same question `sync_key` answers, asked without a pass.

mod common;

use std::sync::Arc;

use common::*;
use journal_fs_store::FsStore;
use journal_remote::{MemoryRemote, RemoteError, RemoteStore};
use journal_store::BlobStore;
use journal_sync_engine::{probe_key, KeyProbe, KEY_BLOB};

struct Setup {
    _dir: TempDir,
    local: FsStore,
    remote: Arc<MemoryRemote>,
}

fn setup(local_key: Option<&[u8]>, remote_key: Option<&[u8]>) -> Setup {
    let dir = TempDir::new();
    let local = FsStore::new(dir.path().join("store"));
    let remote = Arc::new(MemoryRemote::new());
    local.put(&n("x.json"), b"entry").unwrap();
    if let Some(bytes) = local_key {
        local.put(&n(KEY_BLOB), bytes).unwrap();
    }
    if let Some(bytes) = remote_key {
        remote
            .put(&n(KEY_BLOB), bytes, journal_remote::Precondition::Absent)
            .unwrap();
    }
    Setup {
        _dir: dir,
        local,
        remote,
    }
}

fn listings(s: &Setup) -> (Vec<String>, Vec<Vec<u8>>) {
    let local = s.local.list().unwrap();
    let bytes = local
        .iter()
        .map(|name| s.local.get(name).unwrap().unwrap())
        .collect();
    let remote = s
        .remote
        .get(&n(KEY_BLOB))
        .unwrap()
        .map(|(b, _)| String::from_utf8_lossy(&b).into_owned());
    (
        local
            .into_iter()
            .map(|n| n.to_string())
            .chain(remote)
            .collect(),
        bytes,
    )
}

#[test]
fn probe_reports_no_remote_key() {
    let s = setup(Some(b"mine"), None);
    assert_eq!(
        probe_key(&s.local, &*s.remote).unwrap(),
        KeyProbe::NoRemoteKey
    );
    let s = setup(None, None);
    assert_eq!(
        probe_key(&s.local, &*s.remote).unwrap(),
        KeyProbe::NoRemoteKey
    );
}

#[test]
fn probe_reports_no_local_key() {
    let s = setup(None, Some(b"theirs"));
    assert_eq!(
        probe_key(&s.local, &*s.remote).unwrap(),
        KeyProbe::NoLocalKey
    );
}

#[test]
fn probe_reports_the_same_key() {
    let s = setup(Some(b"same"), Some(b"same"));
    assert_eq!(probe_key(&s.local, &*s.remote).unwrap(), KeyProbe::Same);
}

#[test]
fn probe_reports_a_different_key() {
    let s = setup(Some(b"mine"), Some(b"theirs"));
    assert_eq!(
        probe_key(&s.local, &*s.remote).unwrap(),
        KeyProbe::Different
    );
}

#[test]
fn probe_writes_nothing() {
    for (l, r) in [
        (Some(&b"a"[..]), None),
        (None, Some(&b"b"[..])),
        (Some(&b"a"[..]), Some(&b"a"[..])),
        (Some(&b"a"[..]), Some(&b"b"[..])),
    ] {
        let s = setup(l, r);
        let before = listings(&s);
        probe_key(&s.local, &*s.remote).unwrap();
        assert_eq!(listings(&s), before);
    }
}

#[test]
fn probe_returns_a_remote_error() {
    let s = setup(Some(b"mine"), Some(b"theirs"));
    s.remote.fail_next(RemoteError::Transient("offline".into()));
    assert!(probe_key(&s.local, &*s.remote).is_err());
}
