//! [`RemoteStore`] over Dropbox's `files` API.
//!
//! Blobs are files directly in the app's own folder — the "App folder" access
//! type (D7) — so the folder is the root (`""`) and a blob is `"/" + name`.

use std::io::Read;
use std::time::Duration;

use dropbox_sdk::client_trait::UserAuthClient;
use dropbox_sdk::default_client::NoauthDefaultClient;
use dropbox_sdk::files::{
    self, DeleteArg, DeleteError, DownloadArg, DownloadError, GetMetadataArg, GetMetadataError,
    ListFolderArg, ListFolderContinueArg, ListFolderContinueError, ListFolderResult, LookupError,
    Metadata, UploadArg, UploadError, WriteError, WriteMode,
};
use dropbox_sdk::types::auth::AuthError;
use dropbox_sdk::Error;
use journal_remote::{
    Change, ChangePage, Cursor, Precondition, RemoteError, RemoteStore, Revision,
};
use journal_store::BlobName;

use crate::client::RefreshingClient;
use crate::hash;
use journal_credential::RefreshToken;

/// Dropbox omits `retry_after` for some write limits and the SDK reads that as
/// zero, which would invite an immediate retry into the same limit.
const MIN_RETRY_AFTER: Duration = Duration::from_secs(1);

/// A Dropbox app folder as a [`RemoteStore`].
///
/// Generic over the SDK client so the tests can script one; production code
/// uses [`DropboxRemote::new`].
pub struct DropboxRemote<C = RefreshingClient<NoauthDefaultClient>> {
    client: C,
}

impl DropboxRemote {
    /// A remote that authenticates as the user who granted `refresh`, using
    /// the app registered under `app_key`.
    pub fn new(app_key: &str, refresh: &RefreshToken) -> Self {
        Self::with_client(RefreshingClient::new(
            NoauthDefaultClient::default(),
            app_key,
            refresh,
        ))
    }
}

impl<C: UserAuthClient> DropboxRemote<C> {
    pub fn with_client(client: C) -> Self {
        Self { client }
    }
}

fn path(name: &BlobName) -> String {
    format!("/{}", name.as_str())
}

impl<C: UserAuthClient + Send> RemoteStore for DropboxRemote<C> {
    fn changes(&self, since: Option<&Cursor>) -> Result<ChangePage, RemoteError> {
        let listing = match since {
            None => files::list_folder(&self.client, &ListFolderArg::new(String::new()))
                .map_err(|e| lift(e).or_other("could not list the Dropbox folder"))?,
            Some(cursor) => files::list_folder_continue(
                &self.client,
                &ListFolderContinueArg::new(cursor.as_str().to_owned()),
            )
            .map_err(|e| match lift(e) {
                Failure::Api(ListFolderContinueError::Reset) => RemoteError::CursorExpired,
                other => other.or_other("could not read Dropbox's changes"),
            })?,
        };
        Ok(change_page(listing))
    }

    fn get(&self, name: &BlobName) -> Result<Option<(Vec<u8>, Revision)>, RemoteError> {
        let downloaded =
            match files::download(&self.client, &DownloadArg::new(path(name)), None, None) {
                Ok(downloaded) => downloaded,
                Err(e) => {
                    return match lift(e) {
                        Failure::Api(DownloadError::Path(LookupError::NotFound)) => Ok(None),
                        other => Err(other.or_other("could not download a blob")),
                    }
                }
            };
        let mut body = downloaded
            .body
            .ok_or_else(|| RemoteError::Other("Dropbox sent no content".to_owned()))?;
        let mut bytes = Vec::new();
        body.read_to_end(&mut bytes)
            .map_err(|e| RemoteError::Transient(format!("download interrupted: {e}")))?;
        Ok(Some((bytes, Revision::new(downloaded.result.rev))))
    }

    fn put(
        &self,
        name: &BlobName,
        bytes: &[u8],
        pre: Precondition,
    ) -> Result<Revision, RemoteError> {
        let mode = match pre {
            Precondition::Absent => WriteMode::Add,
            Precondition::Matches(rev) => WriteMode::Update(rev.as_str().to_owned()),
        };
        // `autorename` off, or a lost race would be settled by Dropbox writing
        // "name (conflicted copy)" beside the blob instead of telling us.
        // `strict_conflict` on, so a `Matches` on a blob that has since been
        // deleted, or replaced by identical bytes, still counts as lost.
        let arg = UploadArg::new(path(name))
            .with_mode(mode)
            .with_autorename(false)
            .with_strict_conflict(true)
            .with_mute(true);
        match files::upload(&self.client, &arg, bytes) {
            Ok(uploaded) => Ok(Revision::new(uploaded.rev)),
            Err(e) => Err(match lift(e) {
                Failure::Api(UploadError::Path(failed)) => match failed.reason {
                    WriteError::Conflict(_) => RemoteError::Conflict { theirs: None },
                    WriteError::TooManyWriteOperations => {
                        RemoteError::Transient("too many write operations".to_owned())
                    }
                    other => RemoteError::Other(format!("could not upload a blob: {other}")),
                },
                other => other.or_other("could not upload a blob"),
            }),
        }
    }

    fn delete(&self, name: &BlobName, pre: Precondition) -> Result<(), RemoteError> {
        match pre {
            Precondition::Matches(rev) => {
                let arg = DeleteArg::new(path(name)).with_parent_rev(rev.as_str().to_owned());
                match files::delete_v2(&self.client, &arg) {
                    Ok(_) => Ok(()),
                    Err(e) => match lift(e) {
                        // Already gone: the caller wanted it gone.
                        Failure::Api(DeleteError::PathLookup(LookupError::NotFound)) => Ok(()),
                        Failure::Api(DeleteError::PathWrite(WriteError::Conflict(_))) => {
                            Err(RemoteError::Conflict { theirs: None })
                        }
                        Failure::Api(DeleteError::TooManyWriteOperations) => Err(
                            RemoteError::Transient("too many write operations".to_owned()),
                        ),
                        other => Err(other.or_other("could not delete a blob")),
                    },
                }
            }
            // Dropbox cannot delete "only if there is nothing there", but that
            // request has nothing to delete: succeed if the blob is absent and
            // refuse if it is not, which is what the precondition says.
            Precondition::Absent => {
                match files::get_metadata(&self.client, &GetMetadataArg::new(path(name))) {
                    Ok(_) => Err(RemoteError::Conflict { theirs: None }),
                    Err(e) => match lift(e) {
                        Failure::Api(GetMetadataError::Path(LookupError::NotFound)) => Ok(()),
                        other => Err(other.or_other("could not look up a blob")),
                    },
                }
            }
        }
    }

    fn content_hash(&self, bytes: &[u8]) -> Option<String> {
        Some(hash::content_hash(bytes))
    }
}

fn change_page(listing: ListFolderResult) -> ChangePage {
    let mut changes = Vec::with_capacity(listing.entries.len());
    for entry in listing.entries {
        match entry {
            Metadata::File(file) => {
                if let Some(name) = blob_name(file.path_lower.as_deref(), &file.name) {
                    changes.push(Change::Upserted {
                        name,
                        rev: Revision::new(file.rev),
                        content_hash: file.content_hash,
                    });
                }
            }
            Metadata::Deleted(deleted) => {
                if let Some(name) = blob_name(deleted.path_lower.as_deref(), &deleted.name) {
                    changes.push(Change::Deleted { name });
                }
            }
            // Blobs are flat: a folder is somebody's, not ours.
            Metadata::Folder(folder) => {
                log::warn!(
                    "ignoring the folder {:?} in the Dropbox app folder",
                    folder.name
                );
            }
        }
    }
    ChangePage {
        changes,
        cursor: Cursor::new(listing.cursor),
        has_more: listing.has_more,
    }
}

/// The blob an entry in the listing names, or `None` for one that is not ours.
///
/// Read from `path_lower` because Dropbox paths ignore case: the listing may
/// show `.Vault-Key` where we wrote `.vault-key`, and the engine compares
/// names exactly. Every name this app writes is already lowercase (UUIDs and
/// `.vault-key`), so the lowercase form is the one it will look for.
fn blob_name(path_lower: Option<&str>, name: &str) -> Option<BlobName> {
    let lower = match path_lower {
        Some(path) => path.strip_prefix('/').unwrap_or(path).to_owned(),
        None => name.to_lowercase(),
    };
    match BlobName::try_from(lower) {
        Ok(name) => Some(name),
        Err(_) => {
            log::warn!("ignoring {name:?} in the Dropbox app folder: not a blob name");
            None
        }
    }
}

/// An SDK error split into what only the call can interpret (`Api`) and what
/// means the same for every call (`Remote`).
pub(crate) enum Failure<E> {
    Api(E),
    Remote(RemoteError),
}

impl<E: std::fmt::Display> Failure<E> {
    /// The [`RemoteError`] for a failure the call has no special case for.
    pub(crate) fn or_other(self, what: &str) -> RemoteError {
        match self {
            Failure::Api(api) => RemoteError::Other(format!("{what}: {api}")),
            Failure::Remote(remote) => remote,
        }
    }
}

pub(crate) fn lift<E: std::error::Error + 'static>(error: Error<E>) -> Failure<E> {
    Failure::Remote(match error {
        Error::Api(api) => return Failure::Api(api),
        // The request never got an answer: the network, not the request.
        Error::HttpClient(e) => RemoteError::Transient(e.to_string()),
        Error::ServerError(message) => {
            RemoteError::Transient(format!("Dropbox server error: {message}"))
        }
        Error::RateLimited {
            retry_after_seconds,
            ..
        } => RemoteError::RateLimited {
            retry_after: Duration::from_secs(retry_after_seconds.into()).max(MIN_RETRY_AFTER),
        },
        // The token was issued before the app gained a scope: reconnecting
        // helps only once the app has it, which the error says.
        Error::Authentication(AuthError::MissingScope(e)) => {
            RemoteError::MissingPermission(e.required_scope)
        }
        // Whatever else the token's trouble (invalid, expired past a
        // refresh), the fix is the same: connect again.
        Error::Authentication(_) => RemoteError::Unauthorized,
        Error::BadRequest(message) => match missing_app_scope(&message) {
            Some(scope) => RemoteError::MissingPermission(scope),
            None => RemoteError::Other(Error::<E>::BadRequest(message).to_string()),
        },
        other @ (Error::Json(_)
        | Error::UnexpectedResponse(_)
        | Error::AccessDenied(_)
        | Error::UnexpectedHttpError { .. }) => RemoteError::Other(other.to_string()),
    })
}

/// The scope named in a 400 that Dropbox sends when the *app* (not only the
/// token) lacks it, as plain text rather than JSON:
///
/// ```text
/// Error in call to API function "files/upload": Your app is not permitted to
/// access this endpoint because it does not have the required scope
/// 'files.content.write'. The owner of the app can enable the scope for the
/// app using the Permissions tab on the App Console.
/// ```
fn missing_app_scope(message: &str) -> Option<String> {
    const MARK: &str = "does not have the required scope '";
    let rest = &message[message.find(MARK)? + MARK.len()..];
    let scope = &rest[..rest.find('\'')?];
    (!scope.is_empty()).then(|| scope.to_owned())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::fake::{wire, FakeHttp, Reply};

    type Remote = DropboxRemote<RefreshingClient<FakeHttp>>;

    /// A remote holding a valid access token, talking to `http`.
    fn remote(http: &FakeHttp) -> Remote {
        DropboxRemote::with_client(RefreshingClient::seeded(http.clone(), None, "tok"))
    }

    fn name(s: &str) -> BlobName {
        BlobName::try_from(s).unwrap()
    }

    fn rev(s: &str) -> Revision {
        Revision::new(s)
    }

    // --- changes ---

    #[test]
    fn the_initial_listing_reads_the_app_folder_root() {
        let http = FakeHttp::default().expect(
            "2/files/list_folder",
            Reply::ok(wire::listing(
                vec![
                    wire::file("a.json", "r1", Some("hash-a")),
                    wire::file(".vault-key", "r2", None),
                ],
                "c1",
                false,
            )),
        );

        let page = remote(&http).changes(None).unwrap();

        assert_eq!(
            page,
            ChangePage {
                changes: vec![
                    Change::Upserted {
                        name: name("a.json"),
                        rev: rev("r1"),
                        content_hash: Some("hash-a".into()),
                    },
                    Change::Upserted {
                        name: name(".vault-key"),
                        rev: rev("r2"),
                        content_hash: None,
                    },
                ],
                cursor: Cursor::new("c1"),
                has_more: false,
            }
        );
        let sent = http.only_request();
        assert_eq!(sent.url, "https://api.dropboxapi.com/2/files/list_folder");
        assert_eq!(sent.header("Authorization"), Some("Bearer tok"));
        // Non-recursive is the API's default, so the SDK leaves it out.
        assert_eq!(sent.args(), json!({ "path": "" }));
    }

    #[test]
    fn continuing_passes_the_cursor_back_and_reports_deletions() {
        let http = FakeHttp::default().expect(
            "2/files/list_folder/continue",
            Reply::ok(wire::listing(
                vec![
                    wire::file("a.json", "r3", Some("h")),
                    wire::deleted("b.json"),
                ],
                "c2",
                true,
            )),
        );

        let page = remote(&http).changes(Some(&Cursor::new("c1"))).unwrap();

        assert_eq!(http.only_request().args(), json!({ "cursor": "c1" }));
        assert_eq!(
            page.changes,
            vec![
                Change::Upserted {
                    name: name("a.json"),
                    rev: rev("r3"),
                    content_hash: Some("h".into()),
                },
                Change::Deleted {
                    name: name("b.json")
                },
            ]
        );
        assert_eq!(page.cursor, Cursor::new("c2"));
        assert!(page.has_more);
    }

    #[test]
    fn pages_chain_through_has_more() {
        let http = FakeHttp::default()
            .expect(
                "2/files/list_folder",
                Reply::ok(wire::listing(vec![wire::file("a", "r1", None)], "c1", true)),
            )
            .expect(
                "2/files/list_folder/continue",
                Reply::ok(wire::listing(
                    vec![wire::file("b", "r2", None)],
                    "c2",
                    false,
                )),
            );
        let remote = remote(&http);

        let first = remote.changes(None).unwrap();
        assert!(first.has_more);
        let second = remote.changes(Some(&first.cursor)).unwrap();

        assert!(!second.has_more);
        assert_eq!(second.cursor, Cursor::new("c2"));
        assert_eq!(http.sent()[1].args(), json!({ "cursor": "c1" }));
        http.assert_finished();
    }

    #[test]
    fn names_come_from_the_lowercased_path() {
        // Dropbox ignores case, and shows whatever casing last wrote the file.
        let http = FakeHttp::default().expect(
            "2/files/list_folder",
            Reply::ok(wire::listing(
                vec![
                    wire::file(".Vault-Key", "r1", None),
                    wire::deleted("ABC.json"),
                ],
                "c",
                false,
            )),
        );

        let page = remote(&http).changes(None).unwrap();

        let names: Vec<_> = page.changes.iter().map(|c| c.name().as_str()).collect();
        assert_eq!(names, [".vault-key", "abc.json"]);
    }

    #[test]
    fn folders_and_foreign_names_are_skipped() {
        let http = FakeHttp::default().expect(
            "2/files/list_folder",
            Reply::ok(wire::listing(
                vec![
                    wire::folder("sub"),
                    // A `:` cannot be a blob name.
                    wire::file("a:b", "r1", None),
                    wire::file("ok.json", "r2", None),
                ],
                "c",
                false,
            )),
        );

        let page = remote(&http).changes(None).unwrap();

        assert_eq!(page.changes.len(), 1);
        assert_eq!(page.changes[0].name().as_str(), "ok.json");
    }

    #[test]
    fn a_reset_cursor_is_reported_as_expired() {
        let http = FakeHttp::default().expect(
            "2/files/list_folder/continue",
            Reply::api_error(wire::tag("reset")),
        );

        let err = remote(&http)
            .changes(Some(&Cursor::new("old")))
            .unwrap_err();

        assert_eq!(err, RemoteError::CursorExpired);
    }

    // --- get ---

    #[test]
    fn get_downloads_the_bytes_and_their_revision() {
        let http = FakeHttp::default().expect(
            "2/files/download",
            Reply::download(wire::file("a.json", "r7", None), b"ciphertext"),
        );

        let got = remote(&http).get(&name("a.json")).unwrap();

        assert_eq!(got, Some((b"ciphertext".to_vec(), rev("r7"))));
        let sent = http.only_request();
        assert_eq!(sent.url, "https://content.dropboxapi.com/2/files/download");
        assert_eq!(sent.args(), json!({ "path": "/a.json" }));
    }

    #[test]
    fn get_of_a_missing_blob_is_none() {
        let http = FakeHttp::default().expect(
            "2/files/download",
            Reply::api_error(json!({ ".tag": "path", "path": wire::tag("not_found") })),
        );

        assert_eq!(remote(&http).get(&name("a.json")).unwrap(), None);
    }

    // --- put ---

    #[test]
    fn put_absent_creates_without_overwriting_or_renaming() {
        let http = FakeHttp::default().expect(
            "2/files/upload",
            Reply::ok(wire::file("a.json", "r1", None)),
        );

        let got = remote(&http)
            .put(&name("a.json"), b"bytes", Precondition::Absent)
            .unwrap();

        assert_eq!(got, rev("r1"));
        let sent = http.only_request();
        assert_eq!(sent.url, "https://content.dropboxapi.com/2/files/upload");
        assert_eq!(sent.body, b"bytes");
        // The SDK omits what equals the API's default: `mode` is `add` and
        // `autorename` is off unless stated, so neither appears.
        assert_eq!(
            sent.args(),
            json!({ "path": "/a.json", "mute": true, "strict_conflict": true })
        );
    }

    #[test]
    fn put_matches_updates_only_that_revision() {
        let http = FakeHttp::default().expect(
            "2/files/upload",
            Reply::ok(wire::file("a.json", "r2", None)),
        );

        let got = remote(&http)
            .put(&name("a.json"), b"new", Precondition::Matches(rev("r1")))
            .unwrap();

        assert_eq!(got, rev("r2"));
        assert_eq!(
            http.only_request().args(),
            json!({
                "path": "/a.json",
                "mode": { ".tag": "update", "update": "r1" },
                "mute": true,
                "strict_conflict": true,
            })
        );
    }

    fn write_failed(reason: serde_json::Value) -> Reply {
        Reply::api_error(json!({
            ".tag": "path",
            "reason": reason,
            "upload_session_id": "s",
        }))
    }

    #[test]
    fn a_refused_write_is_a_conflict_with_nothing_to_hand_back() {
        for pre in [Precondition::Absent, Precondition::Matches(rev("r1"))] {
            let http = FakeHttp::default().expect(
                "2/files/upload",
                write_failed(json!({ ".tag": "conflict", "conflict": wire::tag("file") })),
            );

            let err = remote(&http).put(&name("a"), b"x", pre).unwrap_err();

            assert_eq!(err, RemoteError::Conflict { theirs: None });
        }
    }

    #[test]
    fn other_write_failures_are_not_conflicts() {
        let http = FakeHttp::default().expect(
            "2/files/upload",
            write_failed(wire::tag("too_many_write_operations")),
        );
        let err = remote(&http)
            .put(&name("a"), b"x", Precondition::Absent)
            .unwrap_err();
        assert!(matches!(err, RemoteError::Transient(_)), "{err:?}");

        let http = FakeHttp::default().expect(
            "2/files/upload",
            write_failed(wire::tag("insufficient_space")),
        );
        let err = remote(&http)
            .put(&name("a"), b"x", Precondition::Absent)
            .unwrap_err();
        assert!(matches!(err, RemoteError::Other(_)), "{err:?}");
    }

    // --- delete ---

    fn deleted_ok() -> Reply {
        Reply::ok(json!({ "metadata": wire::deleted("a.json") }))
    }

    #[test]
    fn delete_matches_removes_only_that_revision() {
        let http = FakeHttp::default().expect("2/files/delete_v2", deleted_ok());

        remote(&http)
            .delete(&name("a.json"), Precondition::Matches(rev("r1")))
            .unwrap();

        assert_eq!(
            http.only_request().args(),
            json!({ "path": "/a.json", "parent_rev": "r1" })
        );
    }

    #[test]
    fn deleting_what_is_already_gone_succeeds() {
        let http = FakeHttp::default().expect(
            "2/files/delete_v2",
            Reply::api_error(json!({
                ".tag": "path_lookup",
                "path_lookup": wire::tag("not_found"),
            })),
        );

        remote(&http)
            .delete(&name("a.json"), Precondition::Matches(rev("r1")))
            .unwrap();
    }

    #[test]
    fn a_delete_that_lost_the_race_is_a_conflict() {
        let http = FakeHttp::default().expect(
            "2/files/delete_v2",
            Reply::api_error(json!({
                ".tag": "path_write",
                "path_write": { ".tag": "conflict", "conflict": wire::tag("file") },
            })),
        );

        let err = remote(&http)
            .delete(&name("a.json"), Precondition::Matches(rev("r1")))
            .unwrap_err();

        assert_eq!(err, RemoteError::Conflict { theirs: None });
    }

    #[test]
    fn delete_absent_succeeds_only_when_nothing_is_there() {
        let http = FakeHttp::default().expect(
            "2/files/get_metadata",
            Reply::api_error(json!({ ".tag": "path", "path": wire::tag("not_found") })),
        );
        remote(&http)
            .delete(&name("a.json"), Precondition::Absent)
            .unwrap();
        assert_eq!(http.only_request().args()["path"], "/a.json");

        let http = FakeHttp::default().expect(
            "2/files/get_metadata",
            Reply::ok(wire::file("a.json", "r1", None)),
        );
        let err = remote(&http)
            .delete(&name("a.json"), Precondition::Absent)
            .unwrap_err();
        assert_eq!(err, RemoteError::Conflict { theirs: None });
        // Never fell through to an unconditional delete.
        http.assert_finished();
    }

    // --- content_hash ---

    #[test]
    fn content_hash_is_dropboxs() {
        let http = FakeHttp::default();
        assert_eq!(
            remote(&http).content_hash(b"hello").as_deref(),
            Some("9595c9df90075148eb06860365df33584b75bff782a510c6cd4883a419833d50")
        );
    }

    // --- live ---

    /// Every change since `since`, all pages, and the cursor to continue from.
    fn drain(remote: &impl RemoteStore, since: Option<&Cursor>) -> (Vec<Change>, Cursor) {
        let mut changes = Vec::new();
        let mut page = remote.changes(since).unwrap();
        loop {
            changes.extend(page.changes);
            if !page.has_more {
                return (changes, page.cursor);
            }
            page = remote.changes(Some(&page.cursor)).unwrap();
        }
    }

    /// The scripted tests above prove what this crate sends and how it reads
    /// what comes back. Only Dropbox can say the two agree, and in particular
    /// that a stale `parent_rev` on a delete is refused in the shape
    /// `delete` expects (`path_write/conflict`), so run this against a scratch
    /// app folder before trusting a release.
    ///
    /// `DROPBOX_TOKEN` is an access token for that app, e.g. the one the app
    /// console generates. Blobs are named for a fresh UUID and removed at the
    /// end; a failure part-way leaves them behind.
    #[test]
    #[ignore = "talks to Dropbox: set DROPBOX_TOKEN to an access token for a scratch app folder"]
    fn live_round_trip() {
        let token = std::env::var("DROPBOX_TOKEN").expect("set DROPBOX_TOKEN");
        let remote = DropboxRemote::with_client(RefreshingClient::seeded(
            NoauthDefaultClient::default(),
            None,
            &token,
        ));
        let id = uuid::Uuid::new_v4();
        // The dotted name has to survive too: `.vault-key` is one.
        for blob in [name(&format!("{id}.json")), name(&format!(".live-{id}"))] {
            let (_, cursor) = drain(&remote, None);

            let first = remote.put(&blob, b"v1", Precondition::Absent).unwrap();
            assert_eq!(
                remote.get(&blob).unwrap(),
                Some((b"v1".to_vec(), first.clone()))
            );
            assert_eq!(
                remote.put(&blob, b"other", Precondition::Absent),
                Err(RemoteError::Conflict { theirs: None })
            );

            let second = remote
                .put(&blob, b"v2", Precondition::Matches(first.clone()))
                .unwrap();
            assert_ne!(first, second);
            assert_eq!(
                remote.put(&blob, b"v3", Precondition::Matches(first.clone())),
                Err(RemoteError::Conflict { theirs: None }),
                "a stale revision must be refused, not overwritten"
            );

            let (changes, cursor) = drain(&remote, Some(&cursor));
            assert!(
                changes.contains(&Change::Upserted {
                    name: blob.clone(),
                    rev: second.clone(),
                    content_hash: remote.content_hash(b"v2"),
                }),
                "the feed's hash must equal ours: {changes:?}"
            );

            assert_eq!(
                remote.delete(&blob, Precondition::Matches(first)),
                Err(RemoteError::Conflict { theirs: None }),
                "a stale revision must not delete"
            );
            assert!(remote.get(&blob).unwrap().is_some());

            remote
                .delete(&blob, Precondition::Matches(second.clone()))
                .unwrap();
            assert_eq!(remote.get(&blob).unwrap(), None);
            remote.delete(&blob, Precondition::Matches(second)).unwrap();
            remote.delete(&blob, Precondition::Absent).unwrap();

            let (changes, _) = drain(&remote, Some(&cursor));
            assert!(
                changes.contains(&Change::Deleted { name: blob }),
                "{changes:?}"
            );
        }
    }

    // --- error mapping ---

    /// What `changes` makes of `reply`: the same mapping serves every call.
    fn changes_error(reply: Reply) -> RemoteError {
        let http = FakeHttp::default().expect("2/files/list_folder", reply);
        remote(&http).changes(None).unwrap_err()
    }

    #[test]
    fn a_rate_limit_carries_its_retry_after() {
        let err = changes_error(Reply::status(
            429,
            json!({ "error": { "reason": wire::tag("too_many_requests"), "retry_after": 5 } }),
        ));
        assert_eq!(
            err,
            RemoteError::RateLimited {
                retry_after: Duration::from_secs(5)
            }
        );
    }

    #[test]
    fn a_rate_limit_without_retry_after_still_waits() {
        let err = changes_error(Reply::status(
            429,
            json!({ "error": { "reason": wire::tag("too_many_write_operations") } }),
        ));
        assert_eq!(
            err,
            RemoteError::RateLimited {
                retry_after: MIN_RETRY_AFTER
            }
        );
    }

    #[test]
    fn server_errors_and_a_dead_network_are_transient() {
        let err = changes_error(Reply::status(503, json!("try later")));
        assert!(matches!(err, RemoteError::Transient(_)), "{err:?}");

        let err = changes_error(Reply::network_down());
        assert!(matches!(err, RemoteError::Transient(_)), "{err:?}");
    }

    #[test]
    fn bad_credentials_are_unauthorized() {
        let err = changes_error(Reply::status(
            401,
            json!({ "error": wire::tag("invalid_access_token") }),
        ));
        assert_eq!(err, RemoteError::Unauthorized);

        // No refresh token to fall back on (the test client has none), so an
        // expired access token is the end of the road too.
        let err = changes_error(Reply::status(
            401,
            json!({ "error": wire::tag("expired_access_token") }),
        ));
        assert_eq!(err, RemoteError::Unauthorized);
    }

    #[test]
    fn a_token_without_a_scope_names_the_missing_permission() {
        let err = changes_error(Reply::status(
            401,
            json!({
                "error_summary": "missing_scope/..",
                "error": { ".tag": "missing_scope", "required_scope": "files.metadata.read" },
            }),
        ));
        assert_eq!(
            err,
            RemoteError::MissingPermission("files.metadata.read".to_owned())
        );
    }

    #[test]
    fn an_app_without_a_scope_names_the_missing_permission() {
        let err = changes_error(Reply::text(
            400,
            "Error in call to API function \"files/list_folder\": Your app is not \
             permitted to access this endpoint because it does not have the required \
             scope 'files.metadata.read'. The owner of the app can enable the scope \
             for the app using the Permissions tab on the App Console.",
        ));
        assert_eq!(
            err,
            RemoteError::MissingPermission("files.metadata.read".to_owned())
        );
        assert!(
            err.to_string().contains("`files.metadata.read`"),
            "the message must name the permission: {err}"
        );
    }

    #[test]
    fn anything_else_is_other_and_says_why() {
        let err = changes_error(Reply::status(400, json!("Error in call to API function")));
        assert!(
            matches!(&err, RemoteError::Other(m) if m.contains("malformed")),
            "{err:?}"
        );

        let err = changes_error(Reply::ok(json!("not a listing")));
        assert!(matches!(err, RemoteError::Other(_)), "{err:?}");

        let err = changes_error(Reply::api_error(
            json!({ ".tag": "path", "path": wire::tag("not_found") }),
        ));
        assert!(
            matches!(&err, RemoteError::Other(m) if m.contains("list")),
            "{err:?}"
        );
    }
}
