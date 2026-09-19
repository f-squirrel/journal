# journal-credential

The login a cloud provider gave the app, and where it is kept — provider-neutral.
`RefreshToken` is the long-lived OAuth credential (it redacts itself in `Debug`),
`Credential` is that token plus the id of the provider that issued it and,
when known, the id of the account it acts as, and
`CredentialStore` is where the one saved credential lives between runs.
`FileCredentialStore` is the file implementation: mode `0600`, written
atomically, provider id on the first line, token on the second, and the
account id (if any) on the third. A file without the third line still loads.

The provider id is stored *with* the token so "which account is connected" and
"what it is logged in with" cannot disagree; the app matches it to a provider
at startup, and uses provider and account together to tell that a new
connection is a *different* remote, whose sync state must not be reused.

It knows nothing of any provider's API — `journal-dropbox` mints and uses the
token, `src-tauri` decides when to save it — and it must live outside the
synced store, or the credential would be uploaded. An OS keychain
implementation can replace the file one without touching anything else.
