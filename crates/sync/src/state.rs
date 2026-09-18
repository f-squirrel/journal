//! The engine's per-device memory of what it last synced.
//!
//! Never uploaded, and kept outside the store it describes: it says what *this
//! device* has seen, which is meaningless anywhere else. Losing it is safe by
//! design — every blob is then compared afresh and disagreements are parked as
//! conflicts, so the worst outcome is extra comparison, not lost text.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use journal_remote::{Cursor, Revision};
use journal_store::BlobName;
use serde::{Deserialize, Serialize};

use crate::digest::Digest;

/// What the remote held for a blob when it last matched the local copy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Base {
    pub rev: Revision,
    /// The SHA-256 of the local bytes at that moment.
    pub hash: Digest,
}

/// The other side of a conflict, held until the resolver can deal with it.
#[derive(Clone, Debug)]
pub(crate) struct Parked {
    /// The revision the remote is at *now*.
    pub rev: Revision,
    /// A digest of the bytes the remote holds *now*.
    ///
    /// Usually the same bytes as `theirs`. They differ on a backend that
    /// writes before it checks (Drive): the remote already holds our bytes and
    /// `theirs` is the version they overwrote. Once the conflict is resolved,
    /// this is what the remote is recorded as holding, so the merged result is
    /// pushed against the right revision.
    pub remote_hash: Digest,
    /// The other device's version.
    pub theirs: Vec<u8>,
}

pub(crate) struct State {
    pub cursor: Option<Cursor>,
    pub bases: BTreeMap<BlobName, Base>,
    pub parked: BTreeMap<BlobName, Parked>,
    path: PathBuf,
}

/// The JSON file. Names are plain strings here because `BlobName` is not
/// serializable — and should not be made so for the sake of one file.
#[derive(Default, Serialize, Deserialize)]
struct Disk {
    cursor: Option<Cursor>,
    blobs: BTreeMap<String, Base>,
    /// Only the index. The parked bytes live in the sibling directory, so a
    /// large blob does not make every state write large.
    conflicts: BTreeMap<String, ParkedIndex>,
}

#[derive(Serialize, Deserialize)]
struct ParkedIndex {
    rev: Revision,
    remote_hash: Digest,
}

impl State {
    /// Reads the state file, or starts empty if there is none — or if it is
    /// unreadable, which is safe (see the module docs) and better than leaving
    /// the user unable to sync until they delete it by hand.
    pub(crate) fn load(path: PathBuf) -> io::Result<Self> {
        let disk = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!(
                    "sync state {} is unreadable ({e}); starting over",
                    path.display()
                );
                Disk::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Disk::default(),
            Err(e) => return Err(e),
        };

        let mut state = State {
            cursor: disk.cursor,
            bases: BTreeMap::new(),
            parked: BTreeMap::new(),
            path,
        };
        for (name, base) in disk.blobs {
            match BlobName::try_from(name) {
                Ok(name) => {
                    state.bases.insert(name, base);
                }
                Err(e) => log::warn!("ignoring sync state for a bad blob name: {e}"),
            }
        }
        for (name, index) in disk.conflicts {
            let Ok(name) = BlobName::try_from(name) else {
                continue;
            };
            // A parked conflict without its bytes cannot be resolved. Dropping
            // it is safe: the next push is refused by the remote, which parks
            // it again.
            match fs::read(state.parked_path(&name)) {
                Ok(theirs) => {
                    let parked = Parked {
                        rev: index.rev,
                        remote_hash: index.remote_hash,
                        theirs,
                    };
                    state.parked.insert(name, parked);
                }
                Err(e) => log::warn!("dropping the parked conflict for {name}: {e}"),
            }
        }
        Ok(state)
    }

    /// Where parked bytes live: `<state file>.parked/`, one file per blob.
    fn parked_dir(&self) -> PathBuf {
        let mut dir = self.path.as_os_str().to_owned();
        dir.push(".parked");
        PathBuf::from(dir)
    }

    fn parked_path(&self, name: &BlobName) -> PathBuf {
        self.parked_dir().join(name.as_str())
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        let disk = Disk {
            cursor: self.cursor.clone(),
            blobs: self
                .bases
                .iter()
                .map(|(name, base)| (name.to_string(), base.clone()))
                .collect(),
            conflicts: self
                .parked
                .iter()
                .map(|(name, parked)| {
                    let index = ParkedIndex {
                        rev: parked.rev.clone(),
                        remote_hash: parked.remote_hash,
                    };
                    (name.to_string(), index)
                })
                .collect(),
        };
        let json = serde_json::to_vec_pretty(&disk).map_err(io::Error::other)?;
        write_atomic(&self.path, &json)
    }

    /// Records that the remote and the local copy agreed on `base`.
    pub(crate) fn record(&mut self, name: &BlobName, base: Base) -> io::Result<()> {
        self.bases.insert(name.clone(), base);
        self.save()
    }

    pub(crate) fn forget(&mut self, name: &BlobName) -> io::Result<()> {
        self.bases.remove(name);
        self.save()
    }

    /// Holds the other side of a conflict. The bytes are written before the
    /// index that refers to them, so the index never points at nothing.
    pub(crate) fn park(&mut self, name: &BlobName, parked: Parked) -> io::Result<()> {
        write_atomic(&self.parked_path(name), &parked.theirs)?;
        self.parked.insert(name.clone(), parked);
        self.save()
    }

    /// Releases a parked conflict, optionally recording a new base in the same
    /// write so there is no moment where neither is on disk.
    pub(crate) fn unpark(&mut self, name: &BlobName, base: Option<Base>) -> io::Result<()> {
        self.parked.remove(name);
        if let Some(base) = base {
            self.bases.insert(name.clone(), base);
        }
        self.save()?;
        // After the save, so a crash leaves a stray file rather than an index
        // entry with no bytes.
        match fs::remove_file(self.parked_path(name)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Deletes the state file at `path` and the parked conflicts beside it, for an
/// app that is disconnecting from its remote. Succeeds if there is none.
///
/// Only bookkeeping goes: the blobs in the local store are untouched, and so is
/// the other side of any conflict that was still parked — which is one more
/// reason to settle conflicts before disconnecting, but not a loss of text the
/// device's own store holds.
pub fn erase_state(path: &Path) -> io::Result<()> {
    // The parked bytes first, so a crash leaves an index without bytes, which
    // `load` already drops, rather than bytes nothing refers to.
    let mut parked = path.as_os_str().to_owned();
    parked.push(".parked");
    match fs::remove_dir_all(PathBuf::from(parked)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Writes a file so that a reader — or a crash — sees the old contents or the
/// new, never half of each: temp file, sync, rename.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);

    let write = || -> io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    };
    if let Err(e) = write().and_then(|()| fs::rename(&temp, path)) {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erasing_removes_the_state_and_its_parked_bytes_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("journal-sync-erase-{}", std::process::id()));
        let path = dir.join("state.json");
        let name = BlobName::try_from("a.json").unwrap();
        let mut state = State::load(path.clone()).unwrap();
        state
            .park(
                &name,
                Parked {
                    rev: Revision::new("1"),
                    remote_hash: Digest::of(b"x"),
                    theirs: b"x".to_vec(),
                },
            )
            .unwrap();
        fs::write(dir.join("unrelated"), b"keep").unwrap();

        erase_state(&path).unwrap();

        assert!(!path.exists());
        assert!(!state.parked_dir().exists());
        assert!(dir.join("unrelated").exists());
        // Nothing left to erase is not a failure.
        erase_state(&path).unwrap();

        fs::remove_dir_all(dir).ok();
    }
}
