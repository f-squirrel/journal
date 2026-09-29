//! Dropbox as a [`Provider`]: the one place the app names its key and its
//! adapter crate.

use std::sync::Arc;

use journal_credential::RefreshToken;
use journal_dropbox::{account_id, authorize_url, finish, revoke, DropboxRemote};
use journal_remote::RemoteStore;

use super::{Connection, Provider};

/// The port the redirect URI names. A fixed one, because Dropbox matches the
/// redirect against URIs registered with the app (register
/// `http://127.0.0.1:53682/` there). (rclone uses the same port for the same
/// purpose, so the two cannot connect at once.)
const REDIRECT_PORT: u16 = 53682;

/// Where the user's own app key (Settings › Sync) is read from, at each use.
pub type KeySource = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Dropbox, under the user's app key or, failing that, the one this build was
/// made with.
///
/// The key is public — PKCE means the app carries no secret — but it names an
/// app registered by whoever ships the build (`DROPBOX_APP_KEY=… make run`) or
/// by the user (Settings › Sync). Neither set: the app runs with sync off and
/// a message saying why when connecting is tried.
pub struct Dropbox {
    chosen: KeySource,
    built: Option<&'static str>,
}

impl Dropbox {
    pub fn new(chosen: KeySource, built: Option<&'static str>) -> Self {
        Self { chosen, built }
    }

    pub fn from_build(chosen: KeySource) -> Self {
        Self::new(
            chosen,
            option_env!("DROPBOX_APP_KEY").filter(|key| !key.is_empty()),
        )
    }

    /// The user's key, else the build's.
    pub(super) fn app_key(&self) -> Option<String> {
        (self.chosen)()
            .filter(|key| !key.is_empty())
            .or_else(|| self.built.map(str::to_owned))
    }

    pub(super) fn key(&self) -> Result<String, String> {
        self.app_key().ok_or_else(|| {
            format!(
                "Dropbox has no app key: enter one in Settings › Sync, or register an app at \
                 dropbox.com/developers (\"App folder\" access), allow the redirect URI \
                 http://127.0.0.1:{REDIRECT_PORT}/, tick the scopes files.metadata.read, \
                 files.content.read, files.content.write and account_info.read, and build \
                 with DROPBOX_APP_KEY set"
            )
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

    /// The registered port, once there is an app to register it with. A build
    /// without a key has nothing to connect, and `begin` says so; it need not
    /// hold a fixed port (possibly in use) to get there.
    fn redirect_port(&self) -> Option<u16> {
        self.app_key().map(|_| REDIRECT_PORT)
    }

    fn configured(&self) -> bool {
        self.app_key().is_some()
    }

    fn begin(&self, redirect_uri: &str) -> Result<Connection, String> {
        let (url, pending) = authorize_url(&self.key()?, redirect_uri);
        Ok(Connection {
            url,
            state: pending.state().to_owned(),
            redeem: Box::new(move |code| finish(pending, code).map_err(|e| e.to_string())),
        })
    }

    fn account(&self, token: &RefreshToken) -> Result<String, String> {
        account_id(&self.key()?, token).map_err(|e| e.to_string())
    }

    fn remote(&self, token: &RefreshToken) -> Result<Arc<dyn RemoteStore>, String> {
        Ok(Arc::new(DropboxRemote::new(&self.key()?, token)))
    }

    fn revoke(&self, token: &RefreshToken) -> Result<(), String> {
        revoke(&self.key()?, token).map_err(|e| e.to_string())
    }
}
