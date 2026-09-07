// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use journal_entry::{Entry, Header};
use journal_storage::Storage;
use tauri::{Manager, State};

#[tauri::command]
fn list_entries(storage: State<Storage>) -> Result<Vec<Header>, String> {
    storage.list_entries().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_entry(storage: State<Storage>, id: String) -> Result<Option<Entry>, String> {
    storage.get_entry(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_entry(storage: State<Storage>, entry: Entry) -> Result<(), String> {
    storage.save_entry(&entry).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_entry(storage: State<Storage>, id: String) -> Result<(), String> {
    storage.delete_entry(&id).map_err(|e| e.to_string())
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
            app.manage(Storage::new(entries_dir));

            if let Some(window) = app.get_webview_window("main") {
                window.maximize()?;
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
