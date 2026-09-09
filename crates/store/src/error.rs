use thiserror::Error;

/// Every way a [`BlobStore`](crate::BlobStore) can fail.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// A name that would address something outside the store, refused by
    /// [`BlobName`](crate::BlobName) before any store sees it. A bug in the
    /// caller rather than anything the user did, but reported rather than
    /// panicked on, because names also arrive from a store's own listing.
    #[error("invalid blob name {0:?}: {1}")]
    InvalidName(String, String),
}
