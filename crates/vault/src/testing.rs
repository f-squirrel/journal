//! Helpers shared by this crate's test modules.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use secrecy::SecretString;

/// A directory name no other test in this process will pick. The counter is
/// crate-wide, so modules cannot collide with each other either.
pub(crate) fn temp_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("journal-vault-test-{}-{n}", std::process::id()))
}

pub(crate) fn passphrase(s: &str) -> SecretString {
    SecretString::from(s.to_string())
}
