# journal-dropbox

Dropbox as a `journal_remote::RemoteStore`. The journal's encrypted blobs live
as files directly in the app's own Dropbox folder (the "App folder" access
type), and the crate answers the sync engine's two questions with Dropbox's
own mechanisms: `list_folder` / `list_folder/continue` for "what changed"
(deletions included), and a revision-conditional upload for "can I write this
without clobbering someone".

It moves ciphertext only and holds no key. Built on Dropbox's official
`dropbox-sdk` with just `dbx_files`, `dbx_users` (one route, to name the
account) and its blocking `ureq` client, so there is
no async runtime in the tree (`cargo tree -p journal-dropbox` shows no `tokio`,
`reqwest` or `hyper`), which is what the synchronous trait wants.

## Pieces

- `DropboxRemote` — the `RemoteStore`. `DropboxRemote::new(app_key, &refresh)`
  for production; generic over the SDK client so tests script one.
- `RefreshingClient` — the SDK client, with the token refresh written here
  rather than taken from the SDK's default client, so refresh can be tested
  against a scripted transport instead of only the live service.
- `authorize_url` / `finish` — OAuth 2 authorization code with PKCE and
  offline access. No client secret. The crate opens no browser and listens on
  no socket: the app shows the URL, catches the redirect, checks that the
  redirect's `state` equals `PendingAuth::state()`, and passes the `code` to
  `finish`.
- `account_id(app_key, &refresh)` — the connected account's stable id
  (`users/get_current_account`), which the app saves with the login so that
  another account of the same provider does not inherit the first one's sync
  state. Needs the `account_info.read` scope.

The app registered with Dropbox needs "App folder" access, the redirect URI
the app listens on (`http://127.0.0.1:53682/`), and four scopes ticked in its
Permissions tab: `files.metadata.read`, `files.content.read`,
`files.content.write` and `account_info.read`. Tokens issued before a scope
was ticked never gain it: reconnect after changing them.
- The refresh token is a `journal_credential::RefreshToken`. Keeping it is
  `journal-credential`'s job, not this crate's: nothing here is Dropbox-specific
  about a saved login.

## Behaviour worth knowing

- **Writes** use `mode: add` for `Absent` and `mode: update{rev}` for
  `Matches`, always with `autorename: false` and `strict_conflict: true`. A
  refused write is `RemoteError::Conflict { theirs: None }`; Dropbox refuses
  atomically, so the other side is still there to `get`.
- **Names** come from the listing's `path_lower`, because Dropbox ignores case
  and every name this app writes is lowercase. Folders and names that are not
  valid blob names are skipped with a warning.
- **`content_hash`** is Dropbox's block SHA-256, computed locally, so the
  engine can compare a never-synced blob with the local copy without
  downloading it.
- **Delete with `Absent`** cannot be a Dropbox call (there is nothing to
  condition on), so it looks the blob up: absent succeeds, present is a
  conflict.
- **Refresh tokens are not rotated** by Dropbox, so the token is saved once,
  at connect time, and never rewritten. The SDK exposes only its own `save()`
  format to get it out; a test pins that format.
- **Errors** map as: authentication failures (including a revoked refresh
  token) → `Unauthorized`; a scope missing from the token (401
  `missing_scope`) or from the app (a plain-text 400 naming it) →
  `MissingPermission(scope)`, whose message says to grant it before
  reconnecting; 429 → `RateLimited` (never less than 1 s); network
  failures and 5xx → `Transient`; `list_folder/continue`'s `reset` →
  `CursorExpired` (the engine then lists everything again); anything else →
  `Other` with the SDK's message.

## Logging

The crate never logs a credential. The SDK does: its `dropbox_sdk::oauth2`
target logs the token endpoint's whole response, access and refresh tokens
included, at `debug`. The app's fixed `Info` level keeps that out; anything
that lowers it must keep that target at `info` or above.

## Tests

The tests need no network: a scripted `HttpClient` asserts each request's
route, JSON arguments and headers and returns canned responses. They cover
every trait method, every error mapping, paging, token refresh (including a
revoked token and a second expiry), the PKCE exchange, and the
content hash against vectors from Dropbox's published algorithm.

`live_round_trip` is `#[ignore]`d: it talks to Dropbox and needs
`DROPBOX_TOKEN`, an access token for a scratch app folder.

```sh
DROPBOX_TOKEN=... cargo test -p journal-dropbox -- --ignored
```

It is the only check that Dropbox agrees with the scripted tests, in
particular that a delete with a stale revision is refused as a conflict.
