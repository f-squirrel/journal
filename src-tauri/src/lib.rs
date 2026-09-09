// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use std::sync::{Arc, Mutex};

use journal_entry::{Entry, Header, Id};
use journal_fs_store::FsStore;
use journal_storage::Storage;
use journal_store::BlobStore;
use journal_vault::Vault;
use secrecy::SecretString;
use tauri::{Manager, Runtime, State};

/// The blob store the journal lives in, plus the unlocked `Storage` over it,
/// if any. `storage` is `None` until `unlock` succeeds, and is cleared again by
/// `lock`.
///
/// The store outlives the lock state because `has_vault` has to answer before
/// there is any passphrase to open one with. It is the only copy of "where the
/// journal is" in the app — the layers below take it as given (ADR 0009).
struct VaultState {
    store: Arc<dyn BlobStore>,
    storage: Option<Storage>,
}

type VaultStateMutex = Mutex<VaultState>;

const LOCKED_ERROR: &str = "vault is locked";

#[tauri::command]
fn has_vault(state: State<VaultStateMutex>) -> Result<bool, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Vault::exists(&*state.store).map_err(|e| e.to_string())
}

#[tauri::command]
fn unlock(state: State<VaultStateMutex>, passphrase: String) -> Result<(), String> {
    let passphrase = SecretString::from(passphrase);
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let storage = Storage::open(state.store.clone(), &passphrase).map_err(|e| e.to_string())?;

    state.storage = Some(storage);
    Ok(())
}

#[tauri::command]
fn lock(state: State<VaultStateMutex>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state.storage = None;
    Ok(())
}

#[tauri::command]
fn list_entries(state: State<VaultStateMutex>) -> Result<Vec<Header>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let storage = state.storage.as_ref().ok_or(LOCKED_ERROR)?;
    storage.list_entries().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_entry(state: State<VaultStateMutex>, id: Id) -> Result<Option<Entry>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let storage = state.storage.as_ref().ok_or(LOCKED_ERROR)?;
    storage.get_entry(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_entry(state: State<VaultStateMutex>, entry: Entry) -> Result<(), String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let storage = state.storage.as_ref().ok_or(LOCKED_ERROR)?;
    storage.save_entry(&entry).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_entry(state: State<VaultStateMutex>, id: Id) -> Result<(), String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let storage = state.storage.as_ref().ok_or(LOCKED_ERROR)?;
    storage.delete_entry(&id).map_err(|e| e.to_string())
}

/// The app's log sink. Without one installed, `log::warn!` anywhere in the
/// workspace compiles down to nothing — notably the "skipped an entry that
/// would not decrypt" warning in `journal_storage::list_entries`, which is
/// the only signal that an entry silently vanished from the list.
///
/// `tauri-plugin-log` rather than a plain stderr subscriber because two of
/// the three target platforms have no stderr a developer will ever see: it
/// routes `Stderr` to logcat on Android and oslog on iOS.
fn logging_plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_log::Builder::new()
        .target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Stderr,
        ))
        .level(log::LevelFilter::Info)
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(logging_plugin())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            has_vault,
            unlock,
            lock,
            list_entries,
            get_entry,
            save_entry,
            delete_entry
        ])
        .setup(|app| {
            let entries_dir = app.path().app_data_dir()?.join("entries");
            app.manage(Mutex::new(VaultState {
                store: Arc::new(FsStore::new(entries_dir)),
                storage: None,
            }));

            if let Some(window) = app.get_webview_window("main") {
                window.maximize()?;
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defect P0-2 names: with no logger installed `log::max_level()` is
    /// `Off`, which is exactly the switch that makes `log::warn!` compile down
    /// to nothing — so entries skipped by `list_entries` vanish with no output.
    /// Must be the only test in this binary that touches the global logger.
    #[test]
    fn logging_is_installed() {
        assert_eq!(log::max_level(), log::LevelFilter::Off);

        let _app = tauri::test::mock_builder()
            .plugin(logging_plugin())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app builds");

        assert_ne!(log::max_level(), log::LevelFilter::Off);
    }
}
