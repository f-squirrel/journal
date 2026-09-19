use std::time::Duration;

use thiserror::Error;

use crate::Revision;

/// Every way a [`RemoteStore`](crate::RemoteStore) call can fail.
///
/// The variants are what the sync engine and its worker *decide* on, not what
/// any one provider reports: an adapter maps its client's errors onto them, and
/// anything that would not change what the caller does lands in
/// [`Other`](RemoteError::Other).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RemoteError {
    /// Another writer won the race, so the write was not applied as asked.
    ///
    /// `theirs` is `Some` when the backend found out only *after* writing
    /// (Drive): there a plain `get` would now return our own bytes, so the
    /// error carries the version that was overwritten. It is `None` when the
    /// write was refused atomically (Dropbox, CloudKit), and the other side is
    /// still on the remote, waiting for a `get`.
    #[error("another writer changed this blob first")]
    Conflict { theirs: Option<(Vec<u8>, Revision)> },

    /// The remote no longer honours the cursor passed to
    /// [`changes`](crate::RemoteStore::changes), so what changed since it cannot
    /// be told. Nothing is wrong with the account: list from scratch
    /// (`changes(None)`) and compare, which finds deletions by absence.
    #[error("the remote's change cursor is no longer valid")]
    CursorExpired,

    /// Credentials revoked, or expired past what a refresh can fix. Retrying
    /// cannot help: stop, and ask the user to reconnect.
    #[error("the remote rejected our credentials; reconnect to continue")]
    Unauthorized,

    /// The app registered with the provider lacks a permission the call needs,
    /// named here (Dropbox's `files.content.write`, say).
    ///
    /// Like [`Unauthorized`](RemoteError::Unauthorized) it stops sync until the
    /// user reconnects, but reconnecting alone does not fix it: the owner of the
    /// app has to grant the permission first, and a login issued before that
    /// never gains it. The message says both, since the user cannot tell from
    /// "reconnect" that anything else is needed.
    #[error("the app is missing the permission `{0}`: turn it on in the app's settings with the provider (Dropbox: the Permissions tab), then connect again")]
    MissingPermission(String),

    /// The provider asked us to slow down. Retry no sooner than `retry_after`.
    #[error("rate limited; retry in {retry_after:?}")]
    RateLimited { retry_after: Duration },

    /// A network failure or a 5xx: nothing is wrong with the request, so back
    /// off and try again.
    #[error("transient remote failure: {0}")]
    Transient(String),

    #[error("remote error: {0}")]
    Other(String),
}
