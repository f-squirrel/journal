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
//! allows, and hands the `code` back to [`finish`]. [`account_id`] then names
//! the account the login is for.

use std::fmt;

use dropbox_sdk::client_trait::{NoauthClient, UserAuthClient};
use dropbox_sdk::default_client::NoauthDefaultClient;
use dropbox_sdk::oauth2::{Authorization, AuthorizeUrlBuilder, Oauth2Type, PkceCode};
use dropbox_sdk::users;
use dropbox_sdk::Error;
use journal_credential::RefreshToken;
use journal_remote::RemoteError;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use crate::client::RefreshingClient;
use crate::remote::lift;

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
/// be allowed the scopes `files.metadata.read`, `files.content.read`,
/// `files.content.write` and `account_info.read`; none is requested here, so Dropbox asks for
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
/// caller's decision (see `journal-credential`).
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

/// Dropbox's stable id for the account `refresh` acts as (`dbid:…`).
///
/// The app keeps it with the login: sync state describes one account's folder,
/// and connecting a *different* account of the same provider must not inherit
/// it. Needs the app's `account_info.read` scope; without it this fails with
/// [`RemoteError::MissingPermission`]. Blocks on the network.
pub fn account_id(app_key: &str, refresh: &RefreshToken) -> Result<String, RemoteError> {
    account_id_with(&RefreshingClient::new(
        NoauthDefaultClient::default(),
        app_key,
        refresh,
    ))
}

fn account_id_with(client: &impl UserAuthClient) -> Result<String, RemoteError> {
    users::get_current_account(client)
        .map(|account| account.account_id)
        .map_err(|e| lift(e).or_other("could not read the Dropbox account"))
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

#[cfg(test)]
mod tests {
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
    fn the_account_id_comes_from_the_current_account() {
        let http = FakeHttp::default().expect(
            "2/users/get_current_account",
            Reply::ok(crate::fake::wire::account("dbid:AAH4f99")),
        );

        let id = account_id_with(&RefreshingClient::seeded(http.clone(), None, "tok")).unwrap();

        assert_eq!(id, "dbid:AAH4f99");
        http.assert_finished();
    }

    #[test]
    fn an_app_without_account_info_read_says_so() {
        let http = FakeHttp::default().expect(
            "2/users/get_current_account",
            Reply::status(
                401,
                json!({
                    "error_summary": "missing_scope/..",
                    "error": { ".tag": "missing_scope", "required_scope": "account_info.read" },
                }),
            ),
        );

        let err = account_id_with(&RefreshingClient::seeded(http, None, "tok")).unwrap_err();

        assert_eq!(
            err,
            RemoteError::MissingPermission("account_info.read".to_owned())
        );
    }

    #[test]
    fn secrets_do_not_print() {
        let (_, pending) = authorize_url("k", "http://localhost/");
        let verifier = pending.verifier.code.clone();

        assert!(!format!("{pending:?}").contains(&verifier));
    }
}
