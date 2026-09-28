//! Entry CRUD, search, and plaintext export commands.

use std::path::Path;

use chrono::{DateTime, Utc};
use journal_api::{Entry, Header, Id, SearchMatch};
use tauri::{Manager, Runtime, State};

use crate::vault::{get as vault, VaultState};
use crate::{blocking, export};

#[tauri::command]
pub(crate) async fn list_entries(state: State<'_, VaultState>) -> Result<Vec<Header>, String> {
    let vault = vault(&state)?;
    blocking(move || vault.list_entries()).await
}

#[tauri::command]
pub(crate) async fn get_entry(
    state: State<'_, VaultState>,
    id: Id,
) -> Result<Option<Entry>, String> {
    let vault = vault(&state)?;
    blocking(move || vault.get_entry(&id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn save_entry(
    state: State<'_, VaultState>,
    entry: Entry,
    expected_updated_at: Option<DateTime<Utc>>,
) -> Result<(), String> {
    let vault = vault(&state)?;
    blocking(move || vault.save_entry_if(&entry, expected_updated_at)).await
}

#[tauri::command]
pub(crate) async fn delete_entry(state: State<'_, VaultState>, id: Id) -> Result<(), String> {
    let vault = vault(&state)?;
    blocking(move || vault.delete_entry(&id)).await
}

/// Decrypts every entry on each call; there is deliberately no search index.
#[tauri::command]
pub(crate) async fn search_entries(
    state: State<'_, VaultState>,
    query: String,
) -> Result<Vec<SearchMatch>, String> {
    let vault = vault(&state)?;
    blocking(move || vault.search(&query)).await
}

/// Writes every entry to an explicitly chosen directory as plaintext.
#[tauri::command]
pub(crate) async fn export_entries<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, VaultState>,
    directory: String,
) -> Result<usize, String> {
    let vault = vault(&state)?;
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let count =
        blocking(move || export::to_directory(&vault, Path::new(&directory), &app_data)).await?;

    // Log only the count: paths and titles may contain journal content.
    log::info!("exported {count} entries as plaintext");
    Ok(count)
}
