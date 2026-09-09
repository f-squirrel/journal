use crate::VaultError;

/// Encryption of a blob's contents, independent of where the blob is kept.
///
/// A trait rather than a concrete type for two reasons. It keeps
/// [`journal-storage`](../journal_storage/index.html) — the layer above —
/// unable to reach into key handling even by accident, since all it can name is
/// `encrypt` and `decrypt`. And it lets that layer's tests run against a
/// trivial stand-in instead of paying scrypt for coverage that has nothing to do
/// with cryptography.
///
/// `Send + Sync` because the app holds an unlocked one for the whole session
/// behind Tauri's managed state.
pub trait Cipher: Send + Sync {
    /// Encrypts `plaintext` so that only this cipher's key can read it back.
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError>;

    /// Decrypts what [`Self::encrypt`] produced.
    ///
    /// Must fail — not return garbage — for bytes this cipher did not write.
    /// Callers rely on that to tell a damaged blob from a readable one.
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, VaultError>;
}
