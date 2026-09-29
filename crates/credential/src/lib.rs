//! The login a cloud provider gave the app, and where it is kept.
//!
//! Provider-neutral: every OAuth provider the app might sync to (Dropbox,
//! Google Drive) hands back a long-lived refresh token, and the app has to keep
//! that token, remember which provider issued it, and never write it anywhere
//! that syncs. That is all this crate does. It knows nothing of any provider's
//! API: the adapters (`journal-dropbox`) mint and use the token, and
//! `src-tauri` decides when to save it.
//!
//! The provider id, the account id and the app id are stored *with* the token,
//! in one file, so that "which account is connected", "which app it was
//! granted to" and "what it is logged in with" cannot disagree.

#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

use std::fmt;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
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

/// A [`RefreshToken`], the provider that issued it, and the account it is for.
#[derive(Clone, Debug)]
pub struct Credential {
    /// The issuing provider's id, e.g. `"dropbox"`. Whatever the app calls its
    /// providers; this crate only stores it. Never contains a newline.
    pub provider: String,
    pub token: RefreshToken,
    /// The provider's stable id for the account the token acts as, when it is
    /// known. `None` for a login saved before accounts were recorded; the app
    /// treats that as "some other account". Never contains a newline.
    pub account: Option<String>,
    /// The id of the provider's app (OAuth client) the token was granted to,
    /// when the provider has one. A token works only with the app that minted
    /// it, so the app uses this and never its current setting. Never contains
    /// a newline.
    pub app: Option<String>,
}

impl Credential {
    /// # Panics
    ///
    /// If `provider` is empty or contains a newline: ids are the app's own
    /// constants, and either would make the stored file unreadable.
    pub fn new(provider: impl Into<String>, token: RefreshToken) -> Self {
        let provider = provider.into();
        assert!(
            is_one_line(&provider),
            "a provider id is one non-empty line, not {provider:?}"
        );
        Self {
            provider,
            token,
            account: None,
            app: None,
        }
    }

    /// The same credential, for the account the provider calls `account`.
    ///
    /// # Panics
    ///
    /// If `account` is empty or contains a newline. Providers' account ids are
    /// neither, and either would make the stored file unreadable.
    pub fn with_account(self, account: impl Into<String>) -> Self {
        let account = account.into();
        assert!(
            is_one_line(&account),
            "an account id is one non-empty line, not {account:?}"
        );
        Self {
            account: Some(account),
            ..self
        }
    }

    /// The same credential for `account`, or `None` if the provider handed
    /// back something that cannot be stored on one line.
    ///
    /// The fallible sibling of [`Self::with_account`], which panics: the
    /// panicking form is for the crate's own `&'static str` ids, this one for
    /// anything that came off a network or off disk.
    pub fn try_with_account(self, account: impl Into<String>) -> Option<Self> {
        let account = account.into();
        is_one_line(&account).then(|| Self {
            account: Some(account),
            ..self
        })
    }

    /// The same credential, granted to the app the provider calls `app`.
    ///
    /// # Panics
    ///
    /// If `app` is empty or contains a newline.
    pub fn with_app(self, app: impl Into<String>) -> Self {
        let app = app.into();
        assert!(
            is_one_line(&app),
            "an app id is one non-empty line, not {app:?}"
        );
        Self {
            app: Some(app),
            ..self
        }
    }

    /// The same credential for `app`, or `None` if it cannot be stored on one
    /// line. The fallible sibling of [`Self::with_app`].
    pub fn try_with_app(self, app: impl Into<String>) -> Option<Self> {
        let app = app.into();
        is_one_line(&app).then(|| Self {
            app: Some(app),
            ..self
        })
    }
}

fn is_one_line(s: &str) -> bool {
    !s.trim().is_empty() && !s.contains(['\n', '\r'])
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
pub enum CredentialStoreError {
    #[error("could not {action} the saved login at {}: {error}", .path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        // Named `error`, not `source`: thiserror treats a field named
        // `source` as the error source even without `#[source]`, which
        // would print the cause a second time as the chain's next link on
        // top of this variant's own interpolation of it.
        error: io::Error,
    },
    #[error(
        "the saved login at {} is not in a format this version reads; connect again",
        .0.display()
    )]
    Unreadable(PathBuf),
    /// For a store implemented outside this crate (an OS keychain, a fake).
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

/// A [`CredentialStore`] that keeps the credential in one file, readable by its
/// owner alone: the provider id on the first line, the token on the second,
/// the account id, when there is one, on the third (empty when only the app is
/// known), and the app id, when there is one, on the fourth.
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

    fn fail(&self, action: &'static str, error: io::Error) -> CredentialStoreError {
        Self::fail_at(action, self.path.clone(), error)
    }

    fn fail_at(action: &'static str, path: PathBuf, error: io::Error) -> CredentialStoreError {
        CredentialStoreError::Io {
            action,
            path,
            error,
        }
    }

    fn remove(path: &Path) -> io::Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
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
        let mut lines = text.lines().map(str::trim);
        let provider = lines.next().filter(|line| !line.is_empty());
        let token = lines.next().filter(|line| !line.is_empty());
        let account = lines.next().filter(|line| !line.is_empty());
        let app = lines.next().filter(|line| !line.is_empty());
        let rest = lines.find(|line| !line.is_empty());
        // Validated before constructing: `str::lines` strips a `\r` only
        // before a `\n` and `trim` only at the ends, so an interior one reaches
        // the constructors' asserts, and a damaged file must be an error, not
        // a panic (release builds abort).
        let credential = match (provider, token, rest) {
            (Some(provider), Some(token), None)
                if is_one_line(provider)
                    && is_one_line(token)
                    && account.is_none_or(is_one_line)
                    && app.is_none_or(is_one_line) =>
            {
                let mut credential = Credential::new(provider, RefreshToken::new(token));
                if let Some(account) = account {
                    credential = credential.with_account(account);
                }
                if let Some(app) = app {
                    credential = credential.with_app(app);
                }
                Some(credential)
            }
            _ => None,
        };
        credential
            .map(Some)
            .ok_or_else(|| CredentialStoreError::Unreadable(self.path.clone()))
    }

    fn save(&self, credential: &Credential) -> Result<(), CredentialStoreError> {
        let temp = self.temp_path();
        let write = || -> io::Result<()> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            // Written whole and renamed into place, so a crash leaves either
            // the old login or the new one and never half of one.
            // A stale scratch file from an interrupted save may still be
            // sitting here, at whatever permissions that run left it with.
            // Remove it first, so the file opened below is always a fresh
            // inode — one no other file descriptor (this process's or
            // another's) could still be holding open at the old, looser
            // permissions — rather than a name that already existed being
            // reused in place.
            Self::remove(&temp)?;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temp)?;
            // Belt and braces on Unix (mode() above already set it at
            // creation, atomically); the only thing that matters on
            // non-Unix, where directory permissions are relied on instead.
            restrict_to_owner(&file)?;
            file.write_all(credential.provider.as_bytes())?;
            file.write_all(b"\n")?;
            file.write_all(credential.token.expose().as_bytes())?;
            if credential.account.is_some() || credential.app.is_some() {
                file.write_all(b"\n")?;
                file.write_all(credential.account.as_deref().unwrap_or("").as_bytes())?;
            }
            if let Some(app) = &credential.app {
                file.write_all(b"\n")?;
                file.write_all(app.as_bytes())?;
            }
            file.sync_all()?;
            fs::rename(&temp, &self.path)
        };
        match write() {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = Self::remove(&temp);
                Err(self.fail("write", error))
            }
        }
    }

    fn clear(&self) -> Result<(), CredentialStoreError> {
        let published = Self::remove(&self.path)
            .map_err(|error| Self::fail_at("remove", self.path.clone(), error));
        let temp = self.temp_path();
        let temporary =
            Self::remove(&temp).map_err(|error| Self::fail_at("remove", temp.clone(), error));
        published.and(temporary)
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
#[cfg_attr(coverage_nightly, coverage(off))]
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
    fn the_account_loads_back_when_there_is_one() {
        let store = store();
        store
            .save(&credential("dropbox", "refresh-1").with_account("dbid:A"))
            .unwrap();

        let loaded = store.load().unwrap().unwrap();

        assert_eq!(loaded.provider, "dropbox");
        assert_eq!(loaded.token.expose(), "refresh-1");
        assert_eq!(loaded.account.as_deref(), Some("dbid:A"));
    }

    #[test]
    fn a_login_saved_without_an_account_still_loads() {
        let store = store();
        store.save(&credential("dropbox", "refresh-1")).unwrap();
        // What a build from before accounts were recorded wrote.
        fs::write(store.path(), "dropbox\nrefresh-1\n").unwrap();

        let loaded = store.load().unwrap().unwrap();

        assert_eq!(loaded.token.expose(), "refresh-1");
        assert_eq!(loaded.account, None);
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
    fn a_failed_save_removes_the_scratch_file_and_returns_the_write_error() {
        let store = store();
        fs::create_dir_all(store.path()).unwrap();

        let error = store
            .save(&credential("dropbox", "simulated-secret"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("could not write the saved login"), "{error}");
        assert!(!store.temp_path().exists(), "scratch file survived");
    }

    #[test]
    fn clear_removes_an_interrupted_scratch_file() {
        let store = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.temp_path(), "simulated-secret").unwrap();

        store.clear().unwrap();

        assert!(!store.temp_path().exists());
    }

    #[test]
    fn clear_still_removes_scratch_when_published_removal_fails() {
        let store = store();
        fs::create_dir_all(store.path()).unwrap();
        fs::write(store.temp_path(), "simulated-secret").unwrap();

        let error = store.clear().unwrap_err().to_string();

        assert!(
            error.contains(&store.path().display().to_string()),
            "{error}"
        );
        assert!(!store.temp_path().exists(), "scratch file survived");
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

        // Nor is a file with more lines than any version wrote.
        fs::write(store.path(), "dropbox\ntoken\naccount\napp\nmore").unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn the_app_round_trips_with_and_without_an_account() {
        let store = store();
        store
            .save(
                &credential("dropbox", "refresh-1")
                    .with_account("dbid:A")
                    .with_app("key-1"),
            )
            .unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.account.as_deref(), Some("dbid:A"));
        assert_eq!(loaded.app.as_deref(), Some("key-1"));

        store
            .save(&credential("dropbox", "refresh-1").with_app("key-1"))
            .unwrap();
        assert_eq!(
            fs::read_to_string(store.path()).unwrap(),
            "dropbox\nrefresh-1\n\nkey-1"
        );
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.account, None);
        assert_eq!(loaded.app.as_deref(), Some("key-1"));
    }

    #[test]
    fn a_file_without_an_app_line_loads_with_no_app() {
        let store = store();
        store.save(&credential("dropbox", "x")).unwrap();
        fs::write(store.path(), "dropbox\nrefresh-1\ndbid:A\n").unwrap();

        let loaded = store.load().unwrap().unwrap();

        assert_eq!(loaded.account.as_deref(), Some("dbid:A"));
        assert_eq!(loaded.app, None);
    }

    #[test]
    fn try_with_app_refuses_an_unusable_id() {
        for bad in ["", "   ", "key\rA", "key\nA"] {
            assert!(
                credential("dropbox", "refresh-1")
                    .try_with_app(bad)
                    .is_none(),
                "{bad:?}"
            );
        }

        let ok = credential("dropbox", "refresh-1")
            .try_with_app("key-1")
            .unwrap();
        assert_eq!(ok.app.as_deref(), Some("key-1"));
        assert_eq!(ok.token.expose(), "refresh-1");
    }

    #[test]
    fn the_app_does_not_unhide_the_token() {
        let debug = format!("{:?}", credential("dropbox", "hunter2").with_app("key-1"));
        assert!(!debug.contains("hunter2"));
    }

    #[test]
    fn a_file_with_an_interior_carriage_return_is_rejected_not_a_panic() {
        let store = store();
        store.save(&credential("dropbox", "x")).unwrap();

        // Provider, token and account in turn: `lines` leaves each of these
        // whole, and `trim` cannot reach the `\r` in the middle.
        for damaged in [
            "drop\rbox\nrefresh-1\n",
            "dropbox\nrefresh\r-1\n",
            "dropbox\nrefresh-1\ndbid\r:A\n",
            "dropbox\nrefresh-1\ndbid:A\nap\rp\n",
        ] {
            fs::write(store.path(), damaged).unwrap();

            let error = store.load().unwrap_err().to_string();

            assert!(error.contains("connect again"), "{damaged:?}: {error}");
        }
    }

    #[test]
    fn try_with_account_refuses_an_unusable_id() {
        for bad in ["", "   ", "dbid:\rA", "dbid:\nA"] {
            assert!(
                credential("dropbox", "refresh-1")
                    .try_with_account(bad)
                    .is_none(),
                "{bad:?}"
            );
        }

        let ok = credential("dropbox", "refresh-1")
            .try_with_account("dbid:A")
            .unwrap();
        assert_eq!(ok.account.as_deref(), Some("dbid:A"));
        assert_eq!(ok.token.expose(), "refresh-1");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_its_owner_alone() {
        use std::io::Read;
        use std::os::unix::fs::PermissionsExt;
        let store = store();

        // Even when a scratch file with looser permissions is in the way,
        // holding junk from an interrupted run. Held open across the save:
        // a reader that already had the stale file open must keep seeing
        // what it opened, never the new secret, which is what tells "the
        // stale file was unlinked and a fresh one created in its place"
        // apart from "the existing file was reused and overwritten". (A
        // same-process `MetadataExt::ino` comparison cannot tell these
        // apart: ext4 was observed here to immediately hand the
        // just-freed inode number back to the very next create in the same
        // directory, so an inode-number check would fail even though the
        // file object is in fact new — see the WP5 hand-off note.)
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.temp_path(), "junk").unwrap();
        fs::set_permissions(store.temp_path(), fs::Permissions::from_mode(0o644)).unwrap();
        let mut still_open = fs::File::open(store.temp_path()).unwrap();

        store.save(&credential("dropbox", "refresh-1")).unwrap();

        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.provider, "dropbox");
        assert_eq!(loaded.token.expose(), "refresh-1");

        let metadata = fs::metadata(store.path()).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);

        let mut still_there = String::new();
        still_open.read_to_string(&mut still_there).unwrap();
        assert_eq!(still_there, "junk");
    }

    #[test]
    fn error_display_strings_match_the_pinned_text() {
        let path = PathBuf::from("/tmp/journal-credential-test/credential");

        let io_error = CredentialStoreError::Io {
            action: "write",
            path: path.clone(),
            error: io::Error::other("disk full"),
        };
        assert_eq!(
            io_error.to_string(),
            "could not write the saved login at /tmp/journal-credential-test/credential: disk full"
        );

        let unreadable = CredentialStoreError::Unreadable(path);
        assert_eq!(
            unreadable.to_string(),
            "the saved login at /tmp/journal-credential-test/credential is not in a format \
             this version reads; connect again"
        );
    }

    #[test]
    fn the_other_variant_wraps_a_foreign_error_transparently() {
        let error = CredentialStoreError::Other(Box::new(io::Error::other("x")));
        assert_eq!(error.to_string(), "x");
    }

    #[test]
    fn an_io_error_does_not_also_report_its_cause_as_a_source() {
        use std::error::Error as _;
        let error = CredentialStoreError::Io {
            action: "write",
            path: PathBuf::from("/tmp/journal-credential-test/credential"),
            error: io::Error::other("disk full"),
        };

        // The cause is already interpolated into the Display text above; a
        // field named `source` would make thiserror report it again as the
        // chain's next link.
        assert!(error.source().is_none());
    }
}
