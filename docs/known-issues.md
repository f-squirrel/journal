# Known issues

Problems that are understood and deliberately left unfixed for now. Each
entry says what the user sees, when it happens, how to get out of it, and
what fixing it would take. When one is fixed, delete its entry.

## A passphrase change would read as a different journal

Connecting compares the sealed `.vault-key` bytes of this device and the cloud
(`probe_key` in `crates/sync-engine`, the same rule `Pass::sync_key` uses), and
a mismatch offers to switch to the cloud's journal. A future passphrase change
re-seals the key, so its bytes change and every other device would read as
holding a different journal. That feature must change `probe_key` and
`sync_key` together before it ships.

## Images: gaps that are accepted for now

Images are stored as immutable encrypted blobs owned by their entry
(`<entry-id>.media-<media-id>`), and are deleted only with that entry.

- **An entry brought back by sync loses its images.** If one device deletes an
  entry while another edits it, the edit wins and the entry returns, but the
  deletion of its images also wins, so they show "Image unavailable".
  Snapshots are lost the same way. A fix would be deferred deletion.
- **Removing an image link does not free space.** The blob stays until the
  entry is deleted. Abandoned inserts and unsaved new entries leave orphans.
- **Hand-pasted `media:` links between entries** break when the owning entry is
  deleted. Conflicted copies are not affected: they copy their images.
- **A sync conflicted copy can keep a link to images not yet pulled**, because
  entry names sort before media names. The same holds for a HEIC, AVIF or TIFF
  whose thumbnail has not arrived yet: the copy waits for it by keeping the
  link to the source entry.
- **Originals are large.** Images are stored as the picked file, so first sync
  and cloud quota grow with the photos (a 12 MP JPEG is 3–12 MB).
- **Import needs a device that can decode the format.** A HEIC or TIFF file is
  refused where the webview cannot read it (HEIC off Apple devices), because
  the thumbnail and dimensions need a decode. Animated GIFs get a still
  thumbnail.
- **Exported images carry their location data.** Export writes the original
  files, EXIF and GPS included; the export dialog says so.
- **iOS caps display copies at about 16.7 MP** (the canvas limit). The
  original is untouched.
- **Images added before thumbnails existed** have none: the read view decrypts
  their original.
- **A failed sync resume says nothing in the UI.** If the saved login cannot
  be resumed at launch (for example a Dropbox login saved before the app key
  was stored with it), sync stays off and the reason is only in the log.
  Settings › Sync then offers Connect, and connecting again fixes it.
