# 0018. Store media as immutable per-entry encrypted blobs

## Status

Accepted — 2026-09-29

## Decision

An image is stored exactly as the user picked it (original bytes and format,
EXIF included) as `<entry-id>.media-<media-id>`, next to a re-encoded
thumbnail (`….thumb`) and, only when the original is HEIC, AVIF or TIFF (not
displayable in every webview), a full-resolution display copy (`….display`).
All three are one `OwnedBlob` kind keyed by `(MediaId, MediaVariant)`,
encrypted like entries, written with `put_new` and never rewritten. The
format is sniffed from the bytes on both sides and `save_media` enforces it.
The read view shows the thumbnail; a full-screen viewer decrypts the full
image only while open. Export writes originals only.

## Reasons

- Immutable blobs need no conflict handling: the sync engine's `immutable`
  shortcut keeps per-pass cost flat, and the resolver keeps either side.
- Folding the variants into `MediaBlob`'s key means the orphan guard, the
  resolver, the immutable predicate and `delete_entry` cover them with no
  further wiring.
- Originals lose nothing; the thumbnail keeps the read view from decrypting
  multi-MB files on every mount, especially on phones.
- Sniffing avoids trusting file names or MIME types (HEIC often has none) and
  refuses SVG.
- Entries own their blobs, so a conflicted copy adopts them by copying
  ciphertext (`Vault::adopt_media`) and deleting the original cannot break it.

## Known costs

Originals are large (first sync and quota grow). Importing needs a device that
can decode the format. Export carries location data. An entry resurrected by
sync loses its images (deletion wins); see `docs/known-issues.md`.

## Alternatives considered

- **Downscale and re-encode to WebP, no original:** small, but drops EXIF and
  exact bytes, and softens screenshots.
- **Original and thumbnail only:** HEIC could not be viewed full size off
  Apple devices.
- **Always store a display copy:** doubles storage for formats every webview
  shows.
- **Separate blob kinds per variant:** each needs its own predicate wired into
  the resolver, engine and deletion.
- **Decode HEIC in Rust (`libheif`):** a new C dependency; the webview already
  decodes what the importing device can show.
