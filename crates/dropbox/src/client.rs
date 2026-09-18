//! The HTTP client the adapter hands to `dropbox-sdk`.
//!
//! The SDK ships a client that keeps its own tokens, but it is welded to its
//! `ureq` transport, so the refresh path — the one part of auth that runs
//! unattended, hours after the user connected — could only be tested against
//! the live service. [`RefreshingClient`] is the same idea over any
//! [`HttpClient`]: the tests script the transport, production uses the SDK's.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dropbox_sdk::client_trait::{HttpClient, HttpRequestResultRaw, NoauthClient, UserAuthClient};
use dropbox_sdk::oauth2::Authorization;
use dropbox_sdk::types::auth::AuthError;
use dropbox_sdk::Error;

use journal_credential::RefreshToken;

/// A user-authorized client that refreshes its own access token.
///
/// The first request finds no access token and fetches one; a later
/// `expired_access_token` response fetches another and retries once, which is
/// the SDK's own loop calling [`update_token`](HttpClient::update_token). A
/// second expiry right after a refresh is returned as the error it is.
pub struct RefreshingClient<T> {
    transport: T,
    tokens: Mutex<Tokens>,
}

struct Tokens {
    /// `None` for a client that holds one access token and cannot renew it.
    auth: Option<Authorization>,
    /// Empty until the first refresh, which is how the SDK asks for one.
    access: Arc<String>,
}

impl<T: HttpClient> RefreshingClient<T> {
    /// A client that will trade `refresh` for access tokens as it needs them.
    pub fn new(transport: T, app_key: &str, refresh: &RefreshToken) -> Self {
        let auth =
            Authorization::from_refresh_token(app_key.to_owned(), refresh.expose().to_owned());
        Self {
            transport,
            tokens: Mutex::new(Tokens {
                auth: Some(auth),
                access: Arc::new(String::new()),
            }),
        }
    }

    /// A client that already holds an access token, with or without the means
    /// to renew it. Tests only: production always starts from a refresh token.
    #[cfg(test)]
    pub(crate) fn seeded(transport: T, auth: Option<Authorization>, access: &str) -> Self {
        Self {
            transport,
            tokens: Mutex::new(Tokens {
                auth,
                access: Arc::new(access.to_owned()),
            }),
        }
    }

    fn tokens(&self) -> MutexGuard<'_, Tokens> {
        // A panic elsewhere must not turn every later request into one.
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: HttpClient> HttpClient for RefreshingClient<T> {
    type Request = T::Request;

    fn execute(&self, request: Self::Request, body: &[u8]) -> Result<HttpRequestResultRaw, Error> {
        self.transport.execute(request, body)
    }

    fn new_request(&self, url: &str) -> Self::Request {
        self.transport.new_request(url)
    }

    fn update_token(&self, old_token: Arc<String>) -> Result<bool, Error> {
        // Held across the request on purpose: a second caller that finds the
        // token already replaced skips its own refresh.
        let mut tokens = self.tokens();
        let Tokens { auth, access } = &mut *tokens;
        let Some(auth) = auth else {
            return Ok(false);
        };
        if **access == *old_token {
            log::info!("refreshing the Dropbox access token");
            match auth.obtain_access_token(Unauthenticated(&self.transport)) {
                Ok(fresh) => *access = Arc::new(fresh),
                Err(e) => return Err(refresh_failure(e)),
            }
        }
        Ok(true)
    }

    fn token(&self) -> Option<Arc<String>> {
        let access = Arc::clone(&self.tokens().access);
        (!access.is_empty()).then_some(access)
    }
}

impl<T: HttpClient> UserAuthClient for RefreshingClient<T> {}

/// The token endpoint answers a refresh token that was revoked or never valid
/// with a plain 400 (`invalid_grant`), which the SDK reports as a malformed
/// request. Left alone that would read as a bug in this crate; it is the user's
/// credentials, so say so.
fn refresh_failure(e: Error) -> Error {
    match e {
        Error::BadRequest(_) => {
            log::warn!("Dropbox refused the refresh token");
            Error::Authentication(AuthError::InvalidAccessToken)
        }
        other => other,
    }
}

/// The transport again, for the one call that must not carry a bearer token:
/// the token request itself.
struct Unauthenticated<'a, T>(&'a T);

impl<T: HttpClient> HttpClient for Unauthenticated<'_, T> {
    type Request = T::Request;

    fn execute(&self, request: Self::Request, body: &[u8]) -> Result<HttpRequestResultRaw, Error> {
        self.0.execute(request, body)
    }

    fn new_request(&self, url: &str) -> Self::Request {
        self.0.new_request(url)
    }
}

impl<T: HttpClient> NoauthClient for Unauthenticated<'_, T> {}

#[cfg(test)]
mod tests {
    use journal_remote::{RemoteError, RemoteStore};
    use serde_json::json;

    use super::*;
    use crate::fake::{wire, FakeHttp, Reply};
    use crate::DropboxRemote;

    const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";

    fn empty_listing() -> Reply {
        Reply::ok(wire::listing(vec![], "c", false))
    }

    fn expired() -> Reply {
        Reply::status(401, json!({ "error": wire::tag("expired_access_token") }))
    }

    fn token(access: &str) -> Reply {
        Reply::ok(json!({ "access_token": access, "token_type": "bearer", "expires_in": 14400 }))
    }

    /// A client that has a (stale) access token and can renew it.
    fn renewable(http: &FakeHttp) -> DropboxRemote<RefreshingClient<FakeHttp>> {
        let auth = Authorization::from_refresh_token("app-key".into(), "refresh-1".into());
        DropboxRemote::with_client(RefreshingClient::seeded(http.clone(), Some(auth), "old"))
    }

    fn bearer(sent: &crate::fake::Sent) -> Option<&str> {
        sent.header("Authorization")
    }

    #[test]
    fn an_expired_access_token_is_refreshed_and_the_request_retried() {
        let http = FakeHttp::default()
            .expect("2/files/list_folder", expired())
            .expect("oauth2/token", token("new"))
            .expect("2/files/list_folder", empty_listing());
        let remote = renewable(&http);

        remote.changes(None).unwrap();

        let sent = http.sent();
        assert_eq!(bearer(&sent[0]), Some("Bearer old"));
        assert_eq!(sent[1].url, TOKEN_URL);
        assert_eq!(bearer(&sent[2]), Some("Bearer new"));
        http.assert_finished();
    }

    #[test]
    fn the_refresh_sends_the_refresh_token_and_no_bearer_or_secret() {
        let http = FakeHttp::default()
            .expect("2/files/list_folder", expired())
            .expect("oauth2/token", token("new"))
            .expect("2/files/list_folder", empty_listing());

        renewable(&http).changes(None).unwrap();

        let refresh = &http.sent()[1];
        let mut form = refresh.form();
        form.sort();
        assert_eq!(
            form,
            [
                ("client_id".to_owned(), "app-key".to_owned()),
                ("grant_type".to_owned(), "refresh_token".to_owned()),
                ("refresh_token".to_owned(), "refresh-1".to_owned()),
            ]
        );
        assert_eq!(bearer(refresh), None);
    }

    #[test]
    fn the_new_token_serves_later_requests_without_another_refresh() {
        let http = FakeHttp::default()
            .expect("2/files/list_folder", expired())
            .expect("oauth2/token", token("new"))
            .expect("2/files/list_folder", empty_listing())
            .expect("2/files/list_folder", empty_listing());
        let remote = renewable(&http);

        remote.changes(None).unwrap();
        remote.changes(None).unwrap();

        assert_eq!(bearer(&http.sent()[3]), Some("Bearer new"));
        http.assert_finished();
    }

    #[test]
    fn the_first_request_fetches_a_token_when_there_is_none() {
        let http = FakeHttp::default()
            .expect("oauth2/token", token("first"))
            .expect("2/files/list_folder", empty_listing());
        let client =
            RefreshingClient::new(http.clone(), "app-key", &RefreshToken::new("refresh-1"));

        DropboxRemote::with_client(client).changes(None).unwrap();

        let sent = http.sent();
        assert_eq!(sent[0].url, TOKEN_URL);
        assert_eq!(bearer(&sent[1]), Some("Bearer first"));
        http.assert_finished();
    }

    #[test]
    fn a_second_expiry_right_after_a_refresh_is_not_retried_forever() {
        let http = FakeHttp::default()
            .expect("2/files/list_folder", expired())
            .expect("oauth2/token", token("new"))
            .expect("2/files/list_folder", expired());

        let err = renewable(&http).changes(None).unwrap_err();

        assert_eq!(err, RemoteError::Unauthorized);
        http.assert_finished();
    }

    #[test]
    fn a_revoked_refresh_token_is_unauthorized() {
        let http = FakeHttp::default()
            .expect("2/files/list_folder", expired())
            .expect(
                "oauth2/token",
                Reply::status(
                    400,
                    json!({ "error": "invalid_grant", "error_description": "refresh token is malformed" }),
                ),
            );

        let err = renewable(&http).changes(None).unwrap_err();

        assert_eq!(err, RemoteError::Unauthorized);
        http.assert_finished();
    }

    #[test]
    fn a_refresh_that_cannot_reach_dropbox_is_transient() {
        let http = FakeHttp::default()
            .expect("2/files/list_folder", expired())
            .expect("oauth2/token", Reply::network_down());

        let err = renewable(&http).changes(None).unwrap_err();

        assert!(matches!(err, RemoteError::Transient(_)), "{err:?}");
    }

    #[test]
    fn a_client_without_a_refresh_token_does_not_try_to_refresh() {
        let http = FakeHttp::default().expect("2/files/list_folder", expired());
        let remote =
            DropboxRemote::with_client(RefreshingClient::seeded(http.clone(), None, "only"));

        let err = remote.changes(None).unwrap_err();

        assert_eq!(err, RemoteError::Unauthorized);
        assert_eq!(http.sent().len(), 1);
    }
}
