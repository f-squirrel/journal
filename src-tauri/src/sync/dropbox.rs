//! Dropbox as a [`Provider`]: the one place the app names its key and its
//! adapter crate.

use std::sync::Arc;

use journal_credential::RefreshToken;
use journal_dropbox::{account_id, authorize_url, finish, DropboxRemote};
use journal_remote::RemoteStore;

use super::{Connection, Provider};

/// Dropbox, under the app key this build was made with.
///
/// The key is public — PKCE means the app carries no secret — but it names an
/// app registered by whoever ships the build, so it is supplied at build time:
/// `DROPBOX_APP_KEY=… make run`. A build without one runs, with sync off and a
/// message saying why when connecting is tried.
pub struct Dropbox {
    app_key: Option<&'static str>,
}

impl Dropbox {
    pub fn new(app_key: Option<&'static str>) -> Self {
        Self { app_key }
    }

    pub fn from_build() -> Self {
        Self::new(option_env!("DROPBOX_APP_KEY").filter(|key| !key.is_empty()))
    }

    fn key(&self) -> Result<&'static str, String> {
        self.app_key.ok_or_else(|| {
            "Dropbox is not set up in this build: register an app at dropbox.com/developers \
             (\"App folder\" access), allow the redirect URI http://127.0.0.1:53682/, tick the \
             scopes files.metadata.read, files.content.read, files.content.write and \
             account_info.read, and build with DROPBOX_APP_KEY set"
                .to_owned()
        })
    }
}

impl Provider for Dropbox {
    fn id(&self) -> &'static str {
        "dropbox"
    }

    fn name(&self) -> &'static str {
        "Dropbox"
    }

    fn begin(&self, redirect_uri: &str) -> Result<Connection, String> {
        let (url, pending) = authorize_url(self.key()?, redirect_uri);
        Ok(Connection {
            url,
            state: pending.state().to_owned(),
            redeem: Box::new(move |code| finish(pending, code).map_err(|e| e.to_string())),
        })
    }

    fn account(&self, token: &RefreshToken) -> Result<String, String> {
        account_id(self.key()?, token).map_err(|e| e.to_string())
    }

    fn remote(&self, token: &RefreshToken) -> Result<Arc<dyn RemoteStore>, String> {
        Ok(Arc::new(DropboxRemote::new(self.key()?, token)))
    }
}
