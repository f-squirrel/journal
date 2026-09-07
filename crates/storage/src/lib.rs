use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JournalEntry {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub markdown: String,
}

fn entry_path(dir: &Path, id: &str) -> std::path::PathBuf {
    dir.join(format!("{id}.json"))
}

/// Lists all entries in `dir`, most recently updated first.
pub fn list_entries(dir: &Path) -> Result<Vec<JournalEntry>, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    let mut entries = Vec::new();
    for file in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let file = file.map_err(|e| e.to_string())?;
        if file.path().extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let contents = fs::read_to_string(file.path()).map_err(|e| e.to_string())?;
        entries.push(serde_json::from_str::<JournalEntry>(&contents).map_err(|e| e.to_string())?);
    }
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(entries)
}

pub fn get_entry(dir: &Path, id: &str) -> Result<Option<JournalEntry>, String> {
    match fs::read_to_string(entry_path(dir, id)) {
        Ok(contents) => Ok(Some(
            serde_json::from_str(&contents).map_err(|e| e.to_string())?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Creates or overwrites the entry with the given id.
pub fn save_entry(dir: &Path, entry: &JournalEntry) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let contents = serde_json::to_string_pretty(entry).map_err(|e| e.to_string())?;
    fs::write(entry_path(dir, &entry.id), contents).map_err(|e| e.to_string())
}

pub fn delete_entry(dir: &Path, id: &str) -> Result<(), String> {
    match fs::remove_file(entry_path(dir, id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir() -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "journal-storage-test-{}-{n}",
            std::process::id()
        ))
    }

    fn sample(id: &str) -> JournalEntry {
        JournalEntry {
            id: id.to_string(),
            title: "Test entry".to_string(),
            updated_at: "2026-09-06".to_string(),
            markdown: "# hello".to_string(),
        }
    }

    #[test]
    fn save_get_list_delete_roundtrip() {
        let dir = temp_dir();

        assert!(list_entries(&dir).unwrap().is_empty());

        save_entry(&dir, &sample("a")).unwrap();
        save_entry(&dir, &sample("b")).unwrap();

        let fetched = get_entry(&dir, "a").unwrap().unwrap();
        assert_eq!(fetched.markdown, "# hello");

        assert_eq!(list_entries(&dir).unwrap().len(), 2);

        delete_entry(&dir, "a").unwrap();
        assert!(get_entry(&dir, "a").unwrap().is_none());
        assert_eq!(list_entries(&dir).unwrap().len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_missing_entry_returns_none() {
        let dir = temp_dir();
        assert!(get_entry(&dir, "missing").unwrap().is_none());
    }
}
