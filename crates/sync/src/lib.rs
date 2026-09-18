//! Replicates the journal's encrypted blobs between the local store and a
//! cloud remote.
//!
//! **Local-first** (ADR 0009): the vault reads and writes only the local store,
//! and nothing on that path ever waits on the network. This crate is the other
//! half — a [`SyncEngine`] that, on request, brings the local `BlobStore` and a
//! [`RemoteStore`](journal_remote::RemoteStore) into agreement.
//!
//! The engine holds no key and moves ciphertext only. What a conflict *means*
//! needs plaintext, so it is a hook ([`ConflictResolver`]) implemented by the
//! layer that has the key. Retry and scheduling are not here either:
//! [`SyncEngine::sync_once`] runs one pass and returns a typed error, and the
//! caller — the app's worker — decides what to do about it.
//!
//! # How a pass decides
//!
//! Each blob is compared with a *base*: what the remote held when it last
//! matched the local copy, recorded per device (never synced) in a state file.
//! "Changed" always means different from the base — no clocks are compared.
//!
//! | Local | Remote | Action |
//! | --- | --- | --- |
//! | unchanged | unchanged | nothing |
//! | changed or new | unchanged or absent | push |
//! | unchanged | changed or new | pull |
//! | deleted | unchanged | delete remotely |
//! | unchanged | deleted | delete locally |
//! | changed | deleted | keep the edit: push |
//! | deleted | changed | keep the edit: pull |
//! | changed | changed | conflict, unless they changed to the same thing |
//!
//! A journal must not lose text, so a delete never outranks an edit, and a
//! conflict is never settled by last-writer-wins. The other side's bytes are
//! parked and the resolver decides. The sealed key blob is the exception (D5):
//! it is never merged, and two different keys stop the pass with
//! [`SyncError::KeyMismatch`].
//!
//! [`SyncEngine::status`] reports which local changes have not reached the
//! remote yet, for the UI's per-entry marks.

mod digest;
mod engine;
mod error;
mod notify;
mod pass;
mod state;
mod status;

pub use engine::{ConflictResolver, Resolution, SyncEngine, SyncReport, KEY_BLOB};
pub use error::{SyncError, SyncErrorKind};
pub use notify::NotifyingStore;
pub use status::SyncStatus;
