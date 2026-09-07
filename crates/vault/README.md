# journal-vault

Passphrase-based encryption for data at rest, via `age`'s scrypt recipient.
Exposes `encrypt`/`decrypt` for arbitrary bytes, plus `create_vault`/
`unlock_vault`/`has_vault` to seal and check a passphrase against a canary
file in a directory. See `docs/adrs/` for why `age`+scrypt was chosen.
