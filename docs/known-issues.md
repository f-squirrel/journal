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
with `SyncError::KeyMismatch` and changes nothing (`crates/sync`, decision D5
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
