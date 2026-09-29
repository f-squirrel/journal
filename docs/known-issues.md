# Known issues

Problems that are understood and deliberately left unfixed for now. Each
entry says what the user sees, when it happens, how to get out of it, and
what fixing it would take. When one is fixed, delete its entry.

## Sync stops when this device and the cloud hold different journals

**What the user sees.** The sync button reports a key mismatch
(`key_mismatch`), and sync stops. The only action offered is Disconnect.

**When it happens.** Each journal has its own key, sealed in the
`.vault-key` blob that syncs with the entries. The sync engine never merges
that blob. If this device and the cloud hold different keys, a pass fails
with `SyncError::KeyMismatch` and changes nothing (`crates/sync-engine`, decision D5
in `.claude/plans/cloud-sync.md`). Two journals cannot be merged, because
neither can decrypt the other's entries. In practice that means one of these
happened:

- A second device chose "Create journal" and was connected afterwards, to a
  cloud that already held the first device's journal. This is the common
  case.
- Two devices each created a journal and were connected to an empty cloud.
  The first to sync uploaded its key.
- The user started over on one device (forgotten passphrase, wiped app data),
  then reconnected to a cloud that still held the old journal.
- The user connected a cloud account that holds some other journal.

It does not happen in normal use. A device that connects from the login
screen before creating a journal downloads the cloud's key.

**Workaround.** First, if this device's journal holds anything you want to
keep, use "Export journal" in the rail to save it as plain markdown files
(the sync button opens the sync dialog, so it is not blocked by the
mismatch). Then disconnect. To have this device join the cloud's journal
instead, quit the app, move this device's `entries/` directory out of the
app's data directory (keep it: it is the only copy of anything written
there), start the app, and connect from the login screen. Then unlock with the
cloud journal's passphrase.

**What a fix would take.** Options considered:

- Refuse at connect time, with a clear message, when this device already has
  a journal and the cloud holds a different one.
- Offer "Use the cloud's journal": move the local journal to a backup
  directory, pull the cloud's, and lock.

Offering "replace the cloud with this device's journal" is ruled out: it
would destroy the entries other devices wrote under the cloud's key.

**Watch for.** A future passphrase change re-seals the key, so the key blob's
bytes change and every other device would hit this mismatch. That feature
needs its own handling before it ships.

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
