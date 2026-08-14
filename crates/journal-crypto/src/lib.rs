//! Encryption for the vault: the backend seam, key slots, object sealing,
//! object naming, and recovery codes.
//!
//! The byte-level design is ratified in
//! `docs/superpowers/specs/2026-08-14-crypto-design.md` and is normative for
//! everything in this crate — formats are not chosen at the call site.
//!
//! M1 ships the `age` backend (spec §5). The native Argon2id + `XChaCha20`
//! backend (spec §6) is the M2 target; [`backend`] is the seam that lets one
//! replace the other. See `docs/superpowers/plans/` for the milestone that
//! fills this crate in.

pub mod backend;

#[cfg(test)]
mod tests {
    use super::backend::{Binding, ObjectKind, OBJECT_ID_LEN};

    #[test]
    fn object_kind_discriminants_are_the_on_disk_values() {
        // These are format constants (spec §4.3) — renumbering them silently
        // would make every existing vault unreadable.
        assert_eq!(ObjectKind::Manifest as u8, 1);
        assert_eq!(ObjectKind::Entry as u8, 2);
        assert_eq!(ObjectKind::Attachment as u8, 3);
    }

    #[test]
    fn binding_is_comparable_so_open_can_reject_a_mismatch() {
        let base = Binding {
            format_major: 1,
            format_minor: 0,
            vault_id: [7; 16],
            kind: ObjectKind::Entry,
            object_id: [9; OBJECT_ID_LEN],
        };
        let other = Binding { object_id: [10; OBJECT_ID_LEN], ..base };
        assert_eq!(base, base);
        assert_ne!(base, other);
    }
}
