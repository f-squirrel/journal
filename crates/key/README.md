# journal-key

The key journal entries are encrypted to: an `age` X25519 keypair, sealed
under the user's passphrase into a `.vault-key` blob in the same store as the
entries. Exposes `Keyring` (`exists`/`create`/`unlock`, plus the `Cipher`
impl over it) and the `Cipher` trait itself, so the layer above encrypts
bytes without seeing key handling.

The passphrase buys exactly one scrypt operation, at unlock; entries are
encrypted to the keypair and cost sub-milliseconds. See ADR 0007 for that
scheme, 0009 for why the sealed key lives in the store, and 0010 for why the
name "vault" belongs to `journal-vault` rather than here.
