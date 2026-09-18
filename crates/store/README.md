# journal-store

Where the bytes live — the abstraction only. `BlobStore` is a flat namespace of
opaque named blobs (`get`/`put`/`delete`/`list`/`contains`), and `BlobName` is
the validated newtype its methods take, so a name cannot address anything
outside the store.

It knows nothing about entries, keys or encryption, and holds no
implementation: `journal-fs-store` is the local one, and every layer above
depends on this trait alone. See ADR 0009.
