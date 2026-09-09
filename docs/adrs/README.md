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
