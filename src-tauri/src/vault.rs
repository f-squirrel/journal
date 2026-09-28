//! Vault lifecycle commands and the application's unlocked-vault state.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

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
