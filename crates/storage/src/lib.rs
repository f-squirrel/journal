use std::fs;
use std::path::PathBuf;

use journal_entry::{Entry, Header};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to (de)serialize entry: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Persists journal entries as one JSON file per entry under a directory.
pub struct Storage {
    dir: PathBuf,
}

impl Storage {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Lists all entry headers, most recently updated first.
    pub fn list_entries(&self) -> Result<Vec<Header>, StorageError> {
        fs::create_dir_all(&self.dir)?;

        let mut headers = Vec::new();
        for file in fs::read_dir(&self.dir)? {
            let file = file?;
            if file.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let contents = fs::read_to_string(file.path())?;
            headers.push(serde_json::from_str::<Header>(&contents)?);
        }
        headers.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(headers)
    }

    pub fn get_entry(&self, id: &str) -> Result<Option<Entry>, StorageError> {
        match fs::read_to_string(self.entry_path(id)) {
            Ok(contents) => Ok(Some(serde_json::from_str(&contents)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Creates or overwrites the entry with the given id.
    pub fn save_entry(&self, entry: &Entry) -> Result<(), StorageError> {
        fs::create_dir_all(&self.dir)?;
        let contents = serde_json::to_string_pretty(entry)?;
        fs::write(self.entry_path(&entry.header.id), contents)?;
        Ok(())
    }

    pub fn delete_entry(&self, id: &str) -> Result<(), StorageError> {
        match fs::remove_file(self.entry_path(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir() -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "journal-storage-test-{}-{n}",
            std::process::id()
        ))
    }

    fn sample(id: &str) -> Entry {
        Entry {
            header: Header {
                id: id.to_string(),
                title: "Test entry".to_string(),
                updated_at: Utc::now(),
            },
            markdown: "# hello".to_string(),
        }
    }

    #[test]
    fn save_get_list_delete_roundtrip() {
        let dir = temp_dir();
        let storage = Storage::new(&dir);

        assert!(storage.list_entries().unwrap().is_empty());

        storage.save_entry(&sample("a")).unwrap();
        storage.save_entry(&sample("b")).unwrap();

        let fetched = storage.get_entry("a").unwrap().unwrap();
        assert_eq!(fetched.markdown, "# hello");

        let headers = storage.list_entries().unwrap();
        assert_eq!(headers.len(), 2);
        assert!(headers.iter().all(|header| header.title == "Test entry"));

        storage.delete_entry("a").unwrap();
        assert!(storage.get_entry("a").unwrap().is_none());
        assert_eq!(storage.list_entries().unwrap().len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_missing_entry_returns_none() {
        let dir = temp_dir();
        let storage = Storage::new(&dir);
        assert!(storage.get_entry("missing").unwrap().is_none());
    }
}
