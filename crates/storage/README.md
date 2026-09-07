# journal-storage

Persists `journal-entry` entries as one `age`-encrypted JSON file per entry
under a directory, via `journal-vault`. Exposes `Storage` with
`list_entries` (headers only, most-recent first)/`get_entry`/`save_entry`/
`delete_entry`; entries that fail to decrypt or parse are skipped and
logged rather than failing the listing.
