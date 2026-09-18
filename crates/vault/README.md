# journal-vault

A journal: `journal-entry` entries kept in a `journal-store` blob store, one
`age`-encrypted JSON blob per entry, under a key from `journal-key`. Exposes
`Vault` with `open` (unlock-or-create, refusing to write a fresh key over
existing entries) and `list_entries` (headers only, most-recent
first)/`get_entry`/`save_entry`/`delete_entry`; entries that fail to decrypt
or parse are skipped and logged rather than failing the listing.
`decode_entry` opens the bytes of an entry blob that is not in the store (the
other side of a sync conflict), and `entry_id` is the inverse of the blob
naming: which entry a `BlobName` holds, or `None` for the sealed key and
foreign names.

This is the layer that knows what an entry is, and the one the rest of the
app means by "the vault". See ADRs 0003, 0009 and 0010.
