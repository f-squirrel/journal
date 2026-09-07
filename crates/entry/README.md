# journal-entry

Core `Entry`/`Header`/`Id` types shared across the backend: a UUID-backed
`Id`, and an `Entry` (`Header` + markdown body) that (de)serializes with the
header fields flattened. No I/O — just the data model.
