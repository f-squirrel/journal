//! A read-only look at whether this device and the cloud hold the same journal.

use journal_remote::RemoteStore;
use journal_store::{BlobName, BlobStore};

use crate::engine::KEY_BLOB;
use crate::SyncError;

/// How the key on this device compares with the key in the cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyProbe {
    /// The cloud holds no key: connecting uploads this device's.
    NoRemoteKey,
    /// This device holds no key: connecting downloads the cloud's.
    NoLocalKey,
    /// Both hold the same sealed bytes.
    Same,
    /// Both hold a key and the bytes differ: two journals.
    Different,
}

/// Reads the key blob from each side and compares the sealed bytes.
///
/// Writes nothing, locally or remotely, and does not touch engine state.
/// "Both absent" is [`KeyProbe::NoRemoteKey`].
///
/// This is the rule `Pass::sync_key` uses to raise
/// [`SyncError::KeyMismatch`]: if one of them changes, the other must follow.
pub fn probe_key(local: &dyn BlobStore, remote: &dyn RemoteStore) -> Result<KeyProbe, SyncError> {
    let name = BlobName::try_from(KEY_BLOB).expect("KEY_BLOB is a valid blob name");
    let theirs = remote.get(&name)?.map(|(bytes, _)| bytes);
    let ours = local.get(&name)?;
    Ok(match (ours, theirs) {
        (_, None) => KeyProbe::NoRemoteKey,
        (None, Some(_)) => KeyProbe::NoLocalKey,
        (Some(a), Some(b)) if a == b => KeyProbe::Same,
        (Some(_), Some(_)) => KeyProbe::Different,
    })
}
