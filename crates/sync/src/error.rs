use thiserror::Error;

use journal_remote::RemoteError;
use journal_store::StoreError;

/// Why a sync pass stopped.
#[derive(Debug, Error)]
pub enum SyncError {
    #[error(transparent)]
    Remote(#[from] RemoteError),
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The engine's own bookkeeping file could not be read or written.
    #[error("sync state: {0}")]
    State(#[from] std::io::Error),
    /// The key on this device and the key in the cloud differ (D5).
    ///
    /// Two devices each created their own journal. Merging them is not
    /// possible — entries sealed under one key cannot be opened with the other —
    /// so sync stops until the user chooses which journal to keep.
    #[error("the journal key on this device differs from the one in the cloud; sync is stopped until you choose which to keep")]
    KeyMismatch,
}

/// What a caller does about a failed pass, without the detail.
///
/// Kept apart from [`SyncError`] because it is what the status reports and the
/// UI branches on, and it has to be `Copy` and comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncErrorKind {
    /// No network, or the provider is down. Back off and retry.
    Offline,
    /// The provider asked us to slow down.
    RateLimited,
    /// Credentials are gone. Retrying cannot help; ask the user to reconnect.
    Unauthorized,
    /// See [`SyncError::KeyMismatch`].
    KeyMismatch,
    Other,
}

impl SyncError {
    pub fn kind(&self) -> SyncErrorKind {
        match self {
            SyncError::Remote(RemoteError::Transient(_)) => SyncErrorKind::Offline,
            SyncError::Remote(RemoteError::RateLimited { .. }) => SyncErrorKind::RateLimited,
            SyncError::Remote(RemoteError::Unauthorized) => SyncErrorKind::Unauthorized,
            SyncError::KeyMismatch => SyncErrorKind::KeyMismatch,
            SyncError::Remote(_) | SyncError::Store(_) | SyncError::State(_) => {
                SyncErrorKind::Other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn every_failure_maps_to_the_kind_a_caller_acts_on() {
        let kind = |e: SyncError| e.kind();
        assert_eq!(
            kind(RemoteError::Transient("x".into()).into()),
            SyncErrorKind::Offline
        );
        assert_eq!(
            kind(
                RemoteError::RateLimited {
                    retry_after: Duration::from_secs(1)
                }
                .into()
            ),
            SyncErrorKind::RateLimited
        );
        assert_eq!(
            kind(RemoteError::Unauthorized.into()),
            SyncErrorKind::Unauthorized
        );
        assert_eq!(kind(SyncError::KeyMismatch), SyncErrorKind::KeyMismatch);
        assert_eq!(
            kind(RemoteError::Other("x".into()).into()),
            SyncErrorKind::Other
        );
        assert_eq!(
            kind(std::io::Error::other("x").into()),
            SyncErrorKind::Other
        );
    }
}
