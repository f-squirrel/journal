// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use journal_storage::JournalEntry;
use tauri::{Manager, State};

struct EntriesDir(std::path::PathBuf);

#[tauri::command]
fn list_entries(dir: State<EntriesDir>) -> Result<Vec<JournalEntry>, String> {
    journal_storage::list_entries(&dir.0)
}

#[tauri::command]
fn get_entry(dir: State<EntriesDir>, id: String) -> Result<Option<JournalEntry>, String> {
    journal_storage::get_entry(&dir.0, &id)
}

#[tauri::command]
fn save_entry(dir: State<EntriesDir>, entry: JournalEntry) -> Result<(), String> {
    journal_storage::save_entry(&dir.0, &entry)
}

#[tauri::command]
fn delete_entry(dir: State<EntriesDir>, id: String) -> Result<(), String> {
    journal_storage::delete_entry(&dir.0, &id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_entries,
            get_entry,
            save_entry,
            delete_entry
        ])
        .setup(|app| {
            let entries_dir = app.path().app_data_dir()?.join("entries");
            app.manage(EntriesDir(entries_dir));

            if let Some(window) = app.get_webview_window("main") {
                window.maximize()?;
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
