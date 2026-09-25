//! Dropbox as a [`RemoteStore`](journal_remote::RemoteStore).
//!
//! [`DropboxRemote`] keeps the journal's encrypted blobs as files in the app's
//! own Dropbox folder (the "App folder" access type), and answers the sync
//! engine's two questions — what changed, and can I write this without
//! clobbering someone — with Dropbox's `list_folder`/`continue` cursor and its
//! revision-conditional upload. It moves ciphertext only and holds no key.
//!
//! [`authorize_url`] and [`finish`] connect an account (OAuth 2 with PKCE, no
//! client secret), and the refresh token that comes back is a
//! [`journal_credential::RefreshToken`], which `journal-credential` keeps. The
//! crate opens no browser and listens on no socket; that is the app's job.
//!
//! Built on Dropbox's official `dropbox-sdk`, on its blocking `ureq` client:
//! no async runtime is involved, matching the trait it implements (D8).
//!
//! # Logging
//!
//! This crate never logs a credential. The SDK, however, logs the token
//! endpoint's whole response — access and refresh token included — at `debug`
//! from its `dropbox_sdk::oauth2` target, so a logger configured for `debug`
//! must keep that target at `info` or above.

#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

mod auth;
mod client;
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod fake;
mod hash;
mod remote;

pub use auth::{account_id, authorize_url, finish, revoke, ConnectError, PendingAuth};
pub use client::RefreshingClient;
pub use remote::DropboxRemote;
