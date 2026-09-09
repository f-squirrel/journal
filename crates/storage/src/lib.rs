use std::fs;
use std::path::PathBuf;

use journal_entry::{Entry, Header, Id};
use secrecy::SecretString;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to (de)serialize entry: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("vault error: {0}")]
    Vault(#[from] journal_vault::VaultError),
}

/// Persists journal entries as one `age`-encrypted JSON file per entry under
/// a directory.
pub struct Storage {
    dir: PathBuf,
    passphrase: SecretString,
}

impl Storage {
    pub fn new(dir: impl Into<PathBuf>, passphrase: SecretString) -> Self {
        Self {
            dir: dir.into(),
            passphrase,
        }
    }

    fn entry_path(&self, id: &Id) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Lists all entry headers, most recently updated first.
    ///
    /// Entries that fail to decrypt or parse are skipped and logged rather
    /// than failing the whole call.
    pub fn list_entries(&self) -> Result<Vec<Header>, StorageError> {
        fs::create_dir_all(&self.dir)?;

        let mut headers = Vec::new();
        for file in fs::read_dir(&self.dir)? {
            let file = file?;
            if file.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let ciphertext = fs::read(file.path())?;
            let header = journal_vault::decrypt(&self.passphrase, &ciphertext)
                .map_err(StorageError::from)
                .and_then(|plaintext| {
                    serde_json::from_slice::<Header>(&plaintext).map_err(StorageError::from)
                });
            match header {
                Ok(header) => headers.push(header),
                Err(e) => {
                    // Rendered before the `warn!` rather than inside it: `age`'s
                    // error `Display` lazily loads its i18n bundle and logs while
                    // doing so, and a sink that holds a lock across formatting its
                    // own record (fern's file target, say) would deadlock on the
                    // reentrant call. Formatting here keeps those records outside.
                    let error = e.to_string();
                    log::warn!(
                        "skipping entry that failed to decrypt or parse: file={}, error={error}",
                        file.path().display(),
                    );
                }
            }
        }
        headers.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(headers)
    }

    pub fn get_entry(&self, id: &Id) -> Result<Option<Entry>, StorageError> {
        match fs::read(self.entry_path(id)) {
            Ok(ciphertext) => {
                let plaintext = journal_vault::decrypt(&self.passphrase, &ciphertext)?;
                Ok(Some(serde_json::from_slice(&plaintext)?))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Creates or overwrites the entry with the given id.
    pub fn save_entry(&self, entry: &Entry) -> Result<(), StorageError> {
        fs::create_dir_all(&self.dir)?;
        let plaintext = serde_json::to_vec(entry)?;
        let ciphertext = journal_vault::encrypt(&self.passphrase, &plaintext)?;
        fs::write(self.entry_path(&entry.header.id), ciphertext)?;
        Ok(())
    }

    pub fn delete_entry(&self, id: &Id) -> Result<(), StorageError> {
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
    use std::sync::Mutex;

    fn temp_dir() -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "journal-storage-test-{}-{n}",
            std::process::id()
        ))
    }

    fn passphrase() -> SecretString {
        SecretString::from("test passphrase".to_string())
    }

    fn sample(id: Id) -> Entry {
        Entry {
            header: Header {
                id,
                title: "Test entry".to_string(),
                updated_at: Utc::now(),
            },
            markdown: "# hello".to_string(),
        }
    }

    #[test]
    fn save_get_list_delete_roundtrip() {
        let dir = temp_dir();
        let storage = Storage::new(&dir, passphrase());

        assert!(storage.list_entries().unwrap().is_empty());

        let a = Id::new();
        let b = Id::new();
        storage.save_entry(&sample(a)).unwrap();
        storage.save_entry(&sample(b)).unwrap();

        let fetched = storage.get_entry(&a).unwrap().unwrap();
        assert_eq!(fetched.markdown, "# hello");

        let headers = storage.list_entries().unwrap();
        assert_eq!(headers.len(), 2);
        assert!(headers.iter().all(|header| header.title == "Test entry"));

        storage.delete_entry(&a).unwrap();
        assert!(storage.get_entry(&a).unwrap().is_none());
        assert_eq!(storage.list_entries().unwrap().len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_missing_entry_returns_none() {
        let dir = temp_dir();
        let storage = Storage::new(&dir, passphrase());
        assert!(storage.get_entry(&Id::new()).unwrap().is_none());
    }

    #[test]
    fn entries_are_encrypted_at_rest() {
        let dir = temp_dir();
        let storage = Storage::new(&dir, passphrase());
        let id = Id::new();
        storage.save_entry(&sample(id)).unwrap();

        let on_disk = fs::read(dir.join(format!("{id}.json"))).unwrap();
        let as_text = String::from_utf8_lossy(&on_disk);
        assert!(!as_text.contains("Test entry"));
        assert!(!as_text.contains("hello"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_entries_skips_corrupt_files_without_failing() {
        let dir = temp_dir();
        let storage = Storage::new(&dir, passphrase());

        storage.save_entry(&sample(Id::new())).unwrap();

        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{}.json", Id::new())), b"not valid age ciphertext").unwrap();

        let headers = storage.list_entries().unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].title, "Test entry");

        fs::remove_dir_all(&dir).ok();
    }

    static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// `log`'s logger is global and install-once, so
    /// `list_entries_logs_the_file_it_skipped` is the only test in this crate
    /// allowed to install one.
    struct CapturingLogger;

    impl log::Log for CapturingLogger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            // Formatted before the lock is taken: formatting a record can emit
            // records of its own, and `CAPTURED` is not reentrant.
            let message = record.args().to_string();
            CAPTURED.lock().unwrap().push(message);
        }

        fn flush(&self) {}
    }

    #[test]
    fn list_entries_logs_the_file_it_skipped() {
        log::set_logger(&CapturingLogger).expect("only test installing a logger");
        log::set_max_level(log::LevelFilter::Trace);

        let dir = temp_dir();
        let storage = Storage::new(&dir, passphrase());
        storage.save_entry(&sample(Id::new())).unwrap();

        let corrupt = dir.join(format!("{}.json", Id::new()));
        fs::write(&corrupt, b"not valid age ciphertext").unwrap();

        assert_eq!(storage.list_entries().unwrap().len(), 1);

        let captured = CAPTURED.lock().unwrap().join("\n");
        assert!(
            captured.contains(&corrupt.display().to_string()),
            "no log record named the skipped file; captured: {captured:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }
}
