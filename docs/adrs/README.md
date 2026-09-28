# Architecture Decision Records

| Number | Title | Status |
| --- | --- | --- |
| [0001](0001-tauri-2-with-rust-backend-and-native-ui.md) | Use Tauri 2, with Rust for the backend and native frameworks for the UI | Accepted |
| [0002](0002-react-and-mui-for-the-ui.md) | Use React and MUI (v6) for the UI | Accepted |
| [0003](0003-file-per-entry-json-storage.md) | Store journal entries as one JSON file per entry | Accepted |
| [0004](0004-playwright-for-ui-testing.md) | Use Playwright for UI testing | Accepted |
| [0005](0005-uuid-backed-entry-ids.md) | Use UUID-backed entry ids generated on the frontend | Accepted |
| [0006](0006-age-scrypt-vault-encryption.md) | Encrypt entries at rest with `age`, using per-file scrypt | Superseded by 0007 |
| [0007](0007-cached-x25519-vault-key.md) | Derive a cached X25519 key at unlock instead of scrypt per file | Accepted |
| [0008](0008-tauri-plugin-log-for-logging.md) | Use `tauri-plugin-log` as the app's log sink | Accepted |
| [0009](0009-blob-store-seam-and-local-first-sync.md) | Layer entry storage over a blob store, and sync local-first | Accepted |
| [0010](0010-name-crates-after-what-they-hold.md) | Name the storage crates after what they hold | Accepted |
| [0011](0011-content-security-policy-and-no-global-tauri.md) | Set a Content-Security-Policy, and stop exposing `window.__TAURI__` | Accepted |
| [0012](0012-cloud-sync-remote-store-and-engine.md) | Sync through a separate `RemoteStore` trait and a base-state engine | Accepted |
| [0013](0013-mui-x-date-pickers-for-the-entry-date.md) | Use `@mui/x-date-pickers` for the entry date control | Accepted |
| [0014](0014-search-by-decrypting-on-demand.md) | Search by decrypting on demand, with no persistent index | Accepted |
| [0015](0015-tinykeys-behind-an-action-registry.md) | Use `tinykeys` behind an action registry for keyboard shortcuts | Accepted |
| [0016](0016-settings-as-device-and-journal-structs.md) | Define settings as a device struct and a journal struct, merged per field | Accepted |
| [0017](0017-entry-version-snapshots-sync-like-entries.md) | Sync entry version snapshots like entries, with no engine changes | Accepted |
