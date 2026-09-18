//! The login a cloud provider gave the app, and where it is kept.
//!
//! Provider-neutral: every OAuth provider the app might sync to (Dropbox,
//! Google Drive) hands back a long-lived refresh token, and the app has to keep
//! that token, remember which provider issued it, and never write it anywhere
//! that syncs. That is all this crate does. It knows nothing of any provider's
//! API: the adapters (`journal-dropbox`) mint and use the token, and
//! `src-tauri` decides when to save it.
//!
//! The provider id is stored *with* the token, in one file, so that "which
//! account is connected" and "what it is logged in with" cannot disagree.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;

/// The long-lived credential: everything needed to act as the user, until they
/// revoke the app.
///
/// Never `Debug`-printed or logged: the type redacts itself, and
/// [`expose`](RefreshToken::expose) is the one place the text comes out.
#[derive(Clone)]
pub struct RefreshToken(SecretString);

impl RefreshToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(SecretString::from(token.into()))
    }

    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for RefreshToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RefreshToken([REDACTED])")
    }
}

/// A [`RefreshToken`] and the provider that issued it.
#[derive(Clone, Debug)]
pub struct Credential {
    /// The issuing provider's id, e.g. `"dropbox"`. Whatever the app calls its
    /// providers; this crate only stores it. Never contains a newline.
    pub provider: String,
    pub token: RefreshToken,
}

impl Credential {
    /// # Panics
    ///
    /// If `provider` is empty or contains a newline: ids are the app's own
    /// constants, and either would make the stored file unreadable.
    pub fn new(provider: impl Into<String>, token: RefreshToken) -> Self {
        let provider = provider.into();
        assert!(
            !provider.is_empty() && !provider.contains('\n'),
            "a provider id is one non-empty line, not {provider:?}"
        );
        Self { provider, token }
    }
}

/// Where the credential is kept between runs.
///
/// A trait because the right place differs by platform and will change: a file
/// for now (D6), the OS keychain or keystore later. The credential is a secret
/// and the store must never be inside the synced blob store, or it would be
/// uploaded. One credential at a time: saving replaces whatever was there,
/// which is how switching provider forgets the old login.
pub trait CredentialStore: Send + Sync {
    /// The saved credential, or `None` if nothing is connected.
    fn load(&self) -> Result<Option<Credential>, CredentialStoreError>;

    fn save(&self, credential: &Credential) -> Result<(), CredentialStoreError>;

    /// Forgets the credential. Succeeds if there was none.
    fn clear(&self) -> Result<(), CredentialStoreError>;
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct CredentialStoreError(String);

/// A [`CredentialStore`] that keeps the credential in one file, readable by its
/// owner alone: the provider id on the first line, the token on the second.
///
/// The refresh token is the only secret kept. The providers we know of do not
/// rotate it, so there is nothing to save after a refresh.
#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    path: PathBuf,
}

impl FileCredentialStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn temp_path(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".tmp");
        PathBuf::from(name)
    }

    fn fail(&self, action: &str, e: io::Error) -> CredentialStoreError {
        CredentialStoreError(format!(
            "could not {action} the saved login at {}: {e}",
            self.path.display()
        ))
    }
}

impl CredentialStore for FileCredentialStore {
    fn load(&self) -> Result<Option<Credential>, CredentialStoreError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(self.fail("read", e)),
        };
        // A blank file is no login. A file that is not blank but not ours
        // (say, one from a build that kept only the token) is an error rather
        // than a guess: the user reconnects, and sync says why it is off.
        if text.trim().is_empty() {
            return Ok(None);
        }
        let credential = text
            .split_once('\n')
            .map(|(provider, token)| (provider.trim(), token.trim()))
            .filter(|(provider, token)| !provider.is_empty() && !token.is_empty())
            .map(|(provider, token)| Credential::new(provider, RefreshToken::new(token)));
        credential.map(Some).ok_or_else(|| {
            CredentialStoreError(format!(
                "the saved login at {} is not in a format this version reads; connect again",
                self.path.display()
            ))
        })
    }

    fn save(&self, credential: &Credential) -> Result<(), CredentialStoreError> {
        let write = || -> io::Result<()> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            // Written whole and renamed into place, so a crash leaves either
            // the old login or the new one and never half of one.
            let temp = self.temp_path();
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp)?;
            // Before the token goes in, and whether or not the scratch file
            // already existed with looser permissions.
            restrict_to_owner(&file)?;
            file.write_all(credential.provider.as_bytes())?;
            file.write_all(b"\n")?;
            file.write_all(credential.token.expose().as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)
        };
        write().map_err(|e| self.fail("write", e))
    }

    fn clear(&self) -> Result<(), CredentialStoreError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(self.fail("remove", e)),
        }
    }
}

#[cfg(unix)]
fn restrict_to_owner(file: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

/// Elsewhere the app's config directory is already per-user.
#[cfg(not(unix))]
fn restrict_to_owner(_file: &fs::File) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn store() -> FileCredentialStore {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "journal-credential-test-{}-{n}",
            std::process::id()
        ));
        // A directory that does not exist yet: saving creates it.
        FileCredentialStore::new(dir.join("config").join("credential"))
    }

    fn credential(provider: &str, token: &str) -> Credential {
        Credential::new(provider, RefreshToken::new(token))
    }

    #[test]
    fn the_token_does_not_print() {
        assert!(!format!("{:?}", RefreshToken::new("hunter2")).contains("hunter2"));
        assert!(!format!("{:?}", credential("dropbox", "hunter2")).contains("hunter2"));
    }

    #[test]
    fn nothing_saved_loads_as_none() {
        assert!(store().load().unwrap().is_none());
    }

    #[test]
    fn a_saved_credential_loads_back_with_its_provider() {
        let store = store();
        store.save(&credential("dropbox", "refresh-1")).unwrap();

        let loaded = store.load().unwrap().unwrap();

        assert_eq!(loaded.provider, "dropbox");
        assert_eq!(loaded.token.expose(), "refresh-1");
    }

    #[test]
    fn saving_again_replaces_the_credential_and_leaves_no_scratch_file() {
        let store = store();
        store.save(&credential("dropbox", "refresh-1")).unwrap();
        store.save(&credential("gdrive", "refresh-2")).unwrap();

        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.provider, "gdrive");
        assert_eq!(loaded.token.expose(), "refresh-2");
        let files: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(files, ["credential"]);
    }

    #[test]
    fn clearing_forgets_the_credential_and_is_idempotent() {
        let store = store();
        store.save(&credential("dropbox", "refresh-1")).unwrap();

        store.clear().unwrap();
        store.clear().unwrap();

        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn a_blank_file_is_no_credential() {
        let store = store();
        store.save(&credential("dropbox", "x")).unwrap();
        fs::write(store.path(), "\n").unwrap();

        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn a_file_in_another_format_is_an_error_not_a_guess() {
        let store = store();
        store.save(&credential("dropbox", "x")).unwrap();
        // The old format: the bare token, no provider.
        fs::write(store.path(), "just-a-token").unwrap();

        let error = store.load().unwrap_err().to_string();

        assert!(error.contains("connect again"), "{error}");
        assert!(!error.contains("just-a-token"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_its_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let store = store();

        // Even when a scratch file with looser permissions is in the way.
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.temp_path(), "old").unwrap();
        fs::set_permissions(store.temp_path(), fs::Permissions::from_mode(0o644)).unwrap();
        store.save(&credential("dropbox", "refresh-1")).unwrap();

        let mode = fs::metadata(store.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
