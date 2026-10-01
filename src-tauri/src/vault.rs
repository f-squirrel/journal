//! Vault lifecycle commands and the application's unlocked-vault state.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use journal_key::Keyring;
use journal_store::BlobStore;
use journal_vault::Vault;
use secrecy::SecretString;
use tauri::State;

use crate::blocking;
use crate::sync::SyncControl;

/// The blob store the journal lives in, plus the unlocked `Vault` over it,
/// if any. The store outlives the lock state so `has_vault` can answer before
/// there is a passphrase.
pub(crate) struct VaultState {
    pub(crate) store: Arc<dyn BlobStore>,
    pub(crate) vault: Mutex<Option<Arc<Vault>>>,
    pub(crate) generation: AtomicU64,
}

impl VaultState {
    pub(crate) fn new(store: Arc<dyn BlobStore>) -> Self {
        Self {
            store,
            vault: Mutex::new(None),
            generation: AtomicU64::new(0),
        }
    }
}

pub(crate) const LOCKED_ERROR: &str = "vault is locked";

/// Recovers the guard from poisoning: the mutex only protects one `Option`,
/// so a panic cannot leave a partially-written invariant behind.
pub(crate) fn lock_state(state: &VaultState) -> MutexGuard<'_, Option<Arc<Vault>>> {
    state.vault.lock().unwrap_or_else(|e| e.into_inner())
}

/// Returns a handle without holding the state mutex during vault I/O.
pub(crate) fn get(state: &VaultState) -> Result<Arc<Vault>, String> {
    lock_state(state)
        .clone()
        .ok_or_else(|| LOCKED_ERROR.to_string())
}

/// Claims a generation for an unlock that is about to start.
pub(crate) fn begin_unlock(state: &VaultState) -> u64 {
    state.generation.fetch_add(1, Ordering::SeqCst) + 1
}

/// Publishes an opened vault unless a newer lock-state intent superseded it.
pub(crate) fn install(state: &VaultState, generation: u64, opened: Vault) -> Result<(), String> {
    let mut vault = lock_state(state);
    if state.generation.load(Ordering::SeqCst) != generation {
        return Err(LOCKED_ERROR.to_string());
    }
    *vault = Some(Arc::new(opened));
    Ok(())
}

/// Clears the vault and invalidates any unlock still in flight.
pub(crate) fn clear(state: &VaultState) {
    let mut vault = lock_state(state);
    state.generation.fetch_add(1, Ordering::SeqCst);
    *vault = None;
}

/// How long switching journals waits for commands still holding the vault.
pub(crate) const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// A vault taken out of [`VaultState`] by [`take_drained`], which can be put
/// back with [`Drained::undo`].
pub(crate) struct Drained {
    vault: Option<Arc<Vault>>,
    /// The generation the take published. If it has moved, someone locked or
    /// unlocked since, and the vault stays out.
    generation: u64,
}

impl Drained {
    /// Puts the vault back, unless the lock state changed since it was taken.
    pub(crate) fn undo(self, state: &VaultState) {
        let mut vault = lock_state(state);
        if state.generation.load(Ordering::SeqCst) == self.generation {
            *vault = self.vault;
        }
    }
}

/// Locks the vault like [`clear`], then waits until no command still holds it:
/// every command that got the vault before the lock has returned.
///
/// Without the wait, a save that began before the lock could land after the
/// journal directory is moved away, recreate it through the store, and leave
/// one blob sealed to the old key in the new journal. On timeout the vault is
/// put back and the call fails.
pub(crate) fn take_drained(state: &VaultState, timeout: Duration) -> Result<Drained, String> {
    let (taken, generation) = {
        let mut vault = lock_state(state);
        let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
        (vault.take(), generation)
    };
    let drained = Drained {
        vault: taken,
        generation,
    };
    let Some(arc) = &drained.vault else {
        return Ok(drained);
    };
    let deadline = Instant::now() + timeout;
    while Arc::strong_count(arc) > 1 {
        if Instant::now() >= deadline {
            drained.undo(state);
            return Err("A save is still running. Try again.".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(drained)
}

#[tauri::command]
pub(crate) async fn has_vault(state: State<'_, VaultState>) -> Result<bool, String> {
    let store = state.store.clone();
    blocking(move || Keyring::exists(&*store)).await
}

#[tauri::command]
pub(crate) async fn unlock(
    state: State<'_, VaultState>,
    sync: State<'_, Arc<SyncControl>>,
    passphrase: String,
) -> Result<(), String> {
    let store = state.store.clone();
    let passphrase = SecretString::from(passphrase);

    // Scrypt is slow, so open without the guard and publish only if no lock
    // superseded this attempt.
    let generation = begin_unlock(&state);
    let opened = blocking(move || Vault::open(store, &passphrase)).await?;
    install(&state, generation, opened)?;

    // A conflict parked while the vault was locked can be settled now.
    sync.kick();
    Ok(())
}

#[tauri::command]
pub(crate) async fn lock(state: State<'_, VaultState>) -> Result<(), String> {
    clear(&state);
    Ok(())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use journal_fs_store::FsStore;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicUsize;

    fn unlocked() -> (VaultState, PathBuf) {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("journal-vault-test-{}-{n}", std::process::id()));
        let store: Arc<dyn BlobStore> = Arc::new(FsStore::new(dir.clone()));
        let vault = Vault::open(store.clone(), &SecretString::from("passphrase".to_string()))
            .expect("a fresh store opens");
        let state = VaultState::new(store);
        *lock_state(&state) = Some(Arc::new(vault));
        (state, dir)
    }

    #[test]
    fn take_drained_waits_for_a_held_vault() {
        let (state, dir) = unlocked();
        let held = get(&state).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(held);
        });
        let drained = take_drained(&state, Duration::from_secs(5)).expect("drains once released");
        release.join().unwrap();
        assert!(get(&state).is_err(), "the vault is locked");
        drained.undo(&state);
        assert!(get(&state).is_ok(), "undo puts it back");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn take_drained_gives_the_vault_back_on_timeout() {
        let (state, dir) = unlocked();
        let _held = get(&state).unwrap();
        let err = take_drained(&state, Duration::from_millis(50))
            .err()
            .unwrap();
        assert_eq!(err, "A save is still running. Try again.");
        assert!(get(&state).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_undo_does_not_reinstall_after_a_newer_lock() {
        let (state, dir) = unlocked();
        let drained = take_drained(&state, Duration::from_secs(1)).unwrap();
        clear(&state);
        drained.undo(&state);
        assert!(
            get(&state).is_err(),
            "someone locked meanwhile: stays locked"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn take_drained_with_no_vault_succeeds_and_its_undo_does_nothing() {
        let state = VaultState::new(Arc::new(FsStore::new(
            std::env::temp_dir().join("journal-vault-test-none"),
        )));
        let drained = take_drained(&state, Duration::from_millis(10)).unwrap();
        drained.undo(&state);
        assert!(get(&state).is_err());
    }
}
