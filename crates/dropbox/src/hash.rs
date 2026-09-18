//! Dropbox's content hash, computed locally.
//!
//! The SDK does not compute it, and the engine needs it to tell whether a blob
//! it has never synced already matches the local copy without downloading it.
//! The algorithm is Dropbox's own (developers.dropbox.com/reference/content-hash):
//! SHA-256 each 4 MiB block, SHA-256 the concatenated digests, hex encode.

use sha2::{Digest, Sha256};

const BLOCK_SIZE: usize = 4 * 1024 * 1024;

pub(crate) fn content_hash(bytes: &[u8]) -> String {
    let mut overall = Sha256::new();
    for block in bytes.chunks(BLOCK_SIZE) {
        overall.update(Sha256::digest(block));
    }
    overall
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors from Dropbox's reference implementation, which is:
    //
    //   hashes = b"".join(sha256(block).digest() for block in blocks_of_4MiB(data))
    //   sha256(hashes).hexdigest()
    //
    // run under Python's hashlib rather than this code, so a mistake in the
    // block handling here cannot agree with itself.

    #[test]
    fn empty_input_has_no_blocks() {
        assert_eq!(
            content_hash(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn one_short_block_is_hashed_twice() {
        assert_eq!(
            content_hash(b"hello"),
            "9595c9df90075148eb06860365df33584b75bff782a510c6cd4883a419833d50"
        );
    }

    #[test]
    fn a_full_block_and_a_remainder_are_hashed_separately() {
        let data: Vec<u8> = (0..BLOCK_SIZE + 1000).map(|i| (i % 251) as u8).collect();
        assert_eq!(
            content_hash(&data),
            "dcb4ba5ff947d11766fb50a93a46724c8cd5d530b3e98c6540b893fcbf4d8f4a"
        );
    }

    #[test]
    fn the_block_boundary_adds_no_empty_block() {
        let data: Vec<u8> = (0..BLOCK_SIZE).map(|i| (i % 251) as u8).collect();
        assert_eq!(
            content_hash(&data),
            "b9654428408015906b44a00935b70af33830aa344b780b0eabd535a133150d04"
        );
    }
}
