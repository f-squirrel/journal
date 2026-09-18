//! Connecting an account: OAuth 2 authorization code with PKCE, and where the
//! result is kept.
//!
//! PKCE means the app carries no client secret: the proof that the code is
//! being redeemed by whoever asked for it is a random verifier that never
//! leaves the process. The scope is offline access, so what comes out is a
//! long-lived *refresh token*; access tokens are short-lived and are minted
//! from it on demand (see [`RefreshingClient`](crate::RefreshingClient)).
//!
//! This module never opens a browser and never listens on a socket. The caller
//! shows the user [`authorize_url`], catches the redirect however its platform
//! allows, and hands the `code` back to [`finish`].

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use dropbox_sdk::client_trait::NoauthClient;
use dropbox_sdk::default_client::NoauthDefaultClient;
use dropbox_sdk::oauth2::{Authorization, AuthorizeUrlBuilder, Oauth2Type, PkceCode};
use dropbox_sdk::Error;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

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

/// A connection that has been started and not finished: what [`finish`] needs
/// to redeem the code, and what the redirect must be checked against.
///
/// Consumed by [`finish`], so a code cannot be redeemed twice.
pub struct PendingAuth {
    app_key: String,
    redirect_uri: String,
    /// The PKCE verifier. Secret until the code is redeemed.
    verifier: PkceCode,
    state: String,
}

impl PendingAuth {
    /// The random value that went out in the URL. The redirect carries it back
    /// as `state`, and the caller must refuse a redirect where it differs:
    /// that is what stops another site from completing *its* authorization
    /// inside this app.
    pub fn state(&self) -> &str {
        &self.state
    }
}

impl fmt::Debug for PendingAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingAuth").finish_non_exhaustive()
    }
}

/// The page to send the user to, and the pending connection to finish with.
///
/// `redirect_uri` must be registered with the Dropbox app, and is sent again
/// when the code is redeemed, where Dropbox requires it to match. The app must
/// be allowed the scopes `files.metadata.read`, `files.content.read` and
/// `files.content.write`; none is requested here, so Dropbox asks for
/// everything the app was registered with.
pub fn authorize_url(app_key: &str, redirect_uri: &str) -> (Url, PendingAuth) {
    let verifier = PkceCode::new();
    let state = Uuid::new_v4().simple().to_string();
    let url = AuthorizeUrlBuilder::new(app_key, &Oauth2Type::PKCE(verifier.clone()))
        .redirect_uri(redirect_uri)
        .state(&state)
        .build();
    let pending = PendingAuth {
        app_key: app_key.to_owned(),
        redirect_uri: redirect_uri.to_owned(),
        verifier,
        state,
    };
    (url, pending)
}

/// Redeems the authorization `code` from the redirect for a refresh token.
///
/// Blocks on the network. The token is returned, not stored: saving it is the
/// caller's decision (see [`TokenStore`]).
pub fn finish(pending: PendingAuth, code: &str) -> Result<RefreshToken, ConnectError> {
    finish_with(NoauthDefaultClient::default(), pending, code)
}

fn finish_with(
    transport: impl NoauthClient,
    pending: PendingAuth,
    code: &str,
) -> Result<RefreshToken, ConnectError> {
    let mut auth = Authorization::from_auth_code(
        pending.app_key,
        Oauth2Type::PKCE(pending.verifier),
        code.to_owned(),
        Some(pending.redirect_uri),
    );
    auth.obtain_access_token(transport).map_err(|e| match e {
        // A 400 from the token endpoint: the code is wrong, used, expired, or
        // the redirect URI differs from the one it was issued for.
        Error::BadRequest(_) => ConnectError::Rejected,
        Error::HttpClient(e) => ConnectError::Network(e.to_string()),
        Error::ServerError(message) => ConnectError::Network(message),
        other => ConnectError::Other(other.to_string()),
    })?;

    // The SDK keeps the refresh token to itself and offers only its own
    // `save()` format, "2&<refresh token>", to get it out. A test pins that
    // format, so an SDK that changes it fails there and not at a user's login.
    auth.save()
        .and_then(|saved| saved.strip_prefix("2&").map(RefreshToken::new))
        .ok_or_else(|| {
            ConnectError::Other(
                "Dropbox issued no refresh token; the app must allow offline access".to_owned(),
            )
        })
}

/// Why a connection could not be completed.
#[derive(Debug, Error)]
pub enum ConnectError {
    /// Dropbox refused the code. Start the connection again.
    #[error("Dropbox did not accept the authorization; try connecting again")]
    Rejected,
    /// Nothing wrong with the code, as far as anyone can tell: try again.
    #[error("could not reach Dropbox: {0}")]
    Network(String),
    #[error("could not connect to Dropbox: {0}")]
    Other(String),
}

/// Where the refresh token is kept between runs.
///
/// A trait because the right place differs by platform and will change: a file
/// for now (D6), the OS keychain or keystore later. The token is a credential
/// and the store must never be inside the synced blob store, or it would be
/// uploaded.
pub trait TokenStore: Send + Sync {
    /// The saved token, or `None` if nothing is connected.
    fn load(&self) -> Result<Option<RefreshToken>, TokenStoreError>;

    fn save(&self, token: &RefreshToken) -> Result<(), TokenStoreError>;

    /// Forgets the token. Succeeds if there was none.
    fn clear(&self) -> Result<(), TokenStoreError>;
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct TokenStoreError(String);

/// A [`TokenStore`] that keeps the token in one file, readable by its owner
/// alone.
///
/// The refresh token is the only thing kept. Dropbox does not rotate it, and
/// the SDK does not expose it after the first exchange in any case, so there is
/// nothing to save after a refresh.
#[derive(Debug, Clone)]
pub struct FileTokenStore {
    path: PathBuf,
}

impl FileTokenStore {
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

    fn fail(&self, action: &str, e: io::Error) -> TokenStoreError {
        TokenStoreError(format!(
            "could not {action} the Dropbox token at {}: {e}",
            self.path.display()
        ))
    }
}

impl TokenStore for FileTokenStore {
    fn load(&self) -> Result<Option<RefreshToken>, TokenStoreError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                let token = text.trim();
                Ok((!token.is_empty()).then(|| RefreshToken::new(token)))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.fail("read", e)),
        }
    }

    fn save(&self, token: &RefreshToken) -> Result<(), TokenStoreError> {
        let write = || -> io::Result<()> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            // Written whole and renamed into place, so a crash leaves either
            // the old token or the new one and never half of one.
            let temp = self.temp_path();
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp)?;
            // Before the token goes in, and whether or not the scratch file
            // already existed with looser permissions.
            restrict_to_owner(&file)?;
            file.write_all(token.expose().as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)
        };
        write().map_err(|e| self.fail("write", e))
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
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

    use serde_json::json;

    use super::*;
    use crate::fake::{FakeHttp, Reply};

    const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";

    fn query(url: &Url) -> Vec<(String, String)> {
        url.query_pairs().into_owned().collect()
    }

    fn param(url: &Url, key: &str) -> Option<String> {
        query(url)
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    #[test]
    fn the_authorize_url_asks_for_offline_access_with_pkce_and_no_secret() {
        let (url, pending) = authorize_url("app-key", "http://localhost:53682/");

        assert_eq!(url.host_str(), Some("www.dropbox.com"));
        assert_eq!(url.path(), "/oauth2/authorize");
        assert_eq!(param(&url, "client_id").as_deref(), Some("app-key"));
        assert_eq!(param(&url, "response_type").as_deref(), Some("code"));
        assert_eq!(param(&url, "token_access_type").as_deref(), Some("offline"));
        assert_eq!(
            param(&url, "redirect_uri").as_deref(),
            Some("http://localhost:53682/")
        );
        assert_eq!(
            param(&url, "code_challenge_method").as_deref(),
            Some("S256")
        );
        assert_eq!(param(&url, "state").as_deref(), Some(pending.state()));
        assert!(param(&url, "client_secret").is_none());
    }

    #[test]
    fn the_verifier_never_appears_in_the_url() {
        let (url, pending) = authorize_url("app-key", "http://localhost:53682/");

        let challenge = param(&url, "code_challenge").expect("a challenge");
        assert_ne!(challenge, pending.verifier.code);
        assert!(!url.as_str().contains(&pending.verifier.code));
    }

    #[test]
    fn every_connection_gets_its_own_state_and_verifier() {
        let (a_url, a) = authorize_url("k", "http://localhost/");
        let (b_url, b) = authorize_url("k", "http://localhost/");

        assert_ne!(a.state(), b.state());
        assert_ne!(
            param(&a_url, "code_challenge"),
            param(&b_url, "code_challenge")
        );
    }

    fn redeemed(refresh: Option<&str>) -> Reply {
        let mut body =
            json!({ "access_token": "sl.short", "token_type": "bearer", "expires_in": 14400 });
        if let Some(refresh) = refresh {
            body["refresh_token"] = json!(refresh);
        }
        Reply::ok(body)
    }

    #[test]
    fn finish_redeems_the_code_with_the_verifier_and_returns_the_refresh_token() {
        let (_, pending) = authorize_url("app-key", "http://localhost:53682/");
        let verifier = pending.verifier.code.clone();
        let http = FakeHttp::default().expect("oauth2/token", redeemed(Some("the-refresh")));

        let token = finish_with(http.clone(), pending, "the-code").unwrap();

        // Pins the SDK's `save()` format that `finish` reads the token out of.
        assert_eq!(token.expose(), "the-refresh");
        let sent = http.only_request();
        assert_eq!(sent.url, TOKEN_URL);
        let mut form = sent.form();
        form.sort();
        // The SDK writes `code_verifier` twice, identically; what matters is
        // that it is there and is ours.
        form.dedup();
        assert_eq!(
            form,
            [
                ("client_id".to_owned(), "app-key".to_owned()),
                ("code".to_owned(), "the-code".to_owned()),
                ("code_verifier".to_owned(), verifier),
                ("grant_type".to_owned(), "authorization_code".to_owned()),
                (
                    "redirect_uri".to_owned(),
                    "http://localhost:53682/".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_refused_code_says_to_start_again() {
        let (_, pending) = authorize_url("k", "http://localhost/");
        let http = FakeHttp::default().expect(
            "oauth2/token",
            Reply::status(400, json!({ "error": "invalid_grant" })),
        );

        let err = finish_with(http, pending, "stale").unwrap_err();

        assert!(matches!(err, ConnectError::Rejected), "{err:?}");
        assert!(!err.to_string().contains("stale"));
    }

    #[test]
    fn an_unreachable_dropbox_is_a_network_error() {
        let (_, pending) = authorize_url("k", "http://localhost/");
        let http = FakeHttp::default().expect("oauth2/token", Reply::network_down());

        let err = finish_with(http, pending, "code").unwrap_err();

        assert!(matches!(err, ConnectError::Network(_)), "{err:?}");
    }

    #[test]
    fn a_response_without_a_refresh_token_is_an_error() {
        let (_, pending) = authorize_url("k", "http://localhost/");
        let http = FakeHttp::default().expect("oauth2/token", redeemed(None));

        let err = finish_with(http, pending, "code").unwrap_err();

        assert!(
            matches!(&err, ConnectError::Other(m) if m.contains("refresh token")),
            "{err:?}"
        );
    }

    #[test]
    fn secrets_do_not_print() {
        let (_, pending) = authorize_url("k", "http://localhost/");
        let verifier = pending.verifier.code.clone();

        assert!(!format!("{pending:?}").contains(&verifier));
        assert!(!format!("{:?}", RefreshToken::new("hunter2")).contains("hunter2"));
    }

    // --- FileTokenStore ---

    fn store() -> FileTokenStore {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("journal-dropbox-test-{}-{n}", std::process::id()));
        // A directory that does not exist yet: saving creates it.
        FileTokenStore::new(dir.join("config").join("dropbox-token"))
    }

    #[test]
    fn nothing_saved_loads_as_none() {
        assert!(store().load().unwrap().is_none());
    }

    #[test]
    fn a_saved_token_loads_back() {
        let store = store();
        store.save(&RefreshToken::new("refresh-1")).unwrap();

        assert_eq!(store.load().unwrap().unwrap().expose(), "refresh-1");
    }

    #[test]
    fn saving_again_replaces_the_token_and_leaves_no_scratch_file() {
        let store = store();
        store.save(&RefreshToken::new("refresh-1")).unwrap();
        store.save(&RefreshToken::new("refresh-2")).unwrap();

        assert_eq!(store.load().unwrap().unwrap().expose(), "refresh-2");
        let files: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(files, ["dropbox-token"]);
    }

    #[test]
    fn clearing_forgets_the_token_and_is_idempotent() {
        let store = store();
        store.save(&RefreshToken::new("refresh-1")).unwrap();

        store.clear().unwrap();
        store.clear().unwrap();

        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn a_blank_file_is_no_token() {
        let store = store();
        store.save(&RefreshToken::new("x")).unwrap();
        fs::write(store.path(), "\n").unwrap();

        assert!(store.load().unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_token_file_is_readable_by_its_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let store = store();

        // Even when a scratch file with looser permissions is in the way.
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.temp_path(), "old").unwrap();
        fs::set_permissions(store.temp_path(), fs::Permissions::from_mode(0o644)).unwrap();
        store.save(&RefreshToken::new("refresh-1")).unwrap();

        let mode = fs::metadata(store.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
