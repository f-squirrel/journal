# journal-fs-store

The local filesystem as a `journal-store` `BlobStore`: `FsStore`, one file per
blob in a single directory. The directory is created on first write, and
writes go through a temp file and a rename so a crash cannot leave a torn
blob; leftover `.tmp` files are skipped by `list`.

Only the app's wiring names it; other crates use it as a dev-dependency for
tests. A cloud backend would be a sibling crate. See ADR 0009.
