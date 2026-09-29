//! A journal: entries kept encrypted in a blob store.
//!
//! This is the layer that knows what an entry *is*: which blob holds it, how it
//! is serialized, which blobs in a store are entries at all, and — because that
//! last question is what makes the answer safe — whether a store is a journal
//! to open or an empty one to create. The layers below it know less on purpose:
//! a [`BlobStore`] knows only where bytes live, and a
//! [`Keyring`](journal_key::Keyring) only how to encrypt them (ADR 0009).

#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

mod media;
mod owned;
mod search;
mod settings;
mod snapshot;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use journal_entry::{
    Entry, Header, Id, ImageFormat, MediaId, MediaMeta, MediaVariant, SearchMatch,
};
use journal_key::{Cipher, KeyError, Keyring};
use journal_settings::SettingsError;
use journal_store::{BlobName, BlobStore, StoreError};
use search::Query;
use secrecy::SecretString;
use thiserror::Error;

use media::MediaBlob;
pub use media::{is_media, media_refs, Media, MediaRef};
pub use settings::SETTINGS_BLOB;
pub use snapshot::is_snapshot;
use snapshot::Snapshot;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
    #[error("failed to (de)serialize entry: {0}")]
    Serde(#[from] serde_json::Error),
    /// Transparent on purpose: `journal-key`'s `#[error]` strings are already
    /// written for the user (P1-2), and a "vault error: " label in front of
    /// one would read as a developer's note wrapping a sentence.
    #[error(transparent)]
    Key(#[from] KeyError),
    /// The entry on disk no longer matches the `updated_at` the caller last
    /// saw: another writer — another device via sync, most likely — changed
    /// or created it since. The text is the contract: `src/utils/errors.ts`
    /// on the frontend matches it verbatim to recognise this case out of the
    /// `Err(String)` an `invoke` call returns.
    #[error("this entry was changed on another device since it was opened")]
    Stale,
    /// A journal setting refused by `validate()`, or (in
    /// `merge_journal_settings`) both sides of a conflict failing to decode.
    /// Transparent for the same reason `Key` is: `journal-settings`' messages
    /// are already user-readable.
    #[error(transparent)]
    Settings(#[from] SettingsError),
    /// The version history asked for by timestamp is gone — already pruned,
    /// say. Distinct from [`Stale`](Self::Stale), which means the *live*
    /// entry moved on, not that an old version no longer exists.
    #[error("that version of the entry is no longer available")]
    HistoryNotFound,
    /// A media reference whose blob is gone: deleted with its entry, or not
    /// synced yet. The text is user-facing.
    #[error("that image is no longer available")]
    MediaNotFound,
    #[error("that image is too large to store")]
    MediaTooLarge,
    /// Metadata that does not describe a storable media item: an unknown kind
    /// or format, or an image with a zero dimension.
    #[error("that kind of file cannot be added to an entry")]
    MediaType,
}

/// The extension every entry blob's name ends in.
///
/// It describes the plaintext inside, not the blob: what is stored is `age`
/// ciphertext wrapping this JSON. Kept as-is from ADR 0003 because renaming it
/// would make existing entries unreachable for no benefit.
const ENTRY_EXT: &str = ".json";

/// The blob an entry with this id lives in.
fn blob_name(id: &Id) -> BlobName {
    BlobName::try_from(format!("{id}{ENTRY_EXT}"))
        .expect("a uuid followed by an extension is a valid blob name")
}

/// The entry a blob holds, or `None` for a blob that is not an entry: the
/// sealed key, or a name from somewhere else.
///
/// The inverse of [`blob_name`], and strict about it: a name that parses as an
/// id but is not the one `blob_name` would write for it (upper-case, say) is
/// not ours.
pub fn entry_id(name: &BlobName) -> Option<Id> {
    let id: Id = name.strip_suffix(ENTRY_EXT)?.parse().ok()?;
    (blob_name(&id) == *name).then_some(id)
}

/// Whether `name` is one of our entry blobs.
///
/// Delegates to [`entry_id`] rather than checking the extension alone, so
/// there is exactly one definition of "is an entry blob" in the crate: the
/// two used to disagree (this one accepted anything ending in `.json`, while
/// `entry_id` demanded a lower-case uuid), which meant a stray `notes.json`
/// counted as an entry here and blocked [`Vault::open`]'s orphan check
/// without ever showing up in a listing.
fn is_entry(name: &BlobName) -> bool {
    entry_id(name).is_some()
}

/// A journal: the entries in one blob store, each encrypted at rest under the
/// store's own key.
///
/// The type the rest of the app means by "vault" — what `unlock` produces and
/// `lock` drops. The key itself is a [`Keyring`](journal_key::Keyring) one
/// crate down, which this type holds only as an opaque
/// [`Cipher`](journal_key::Cipher).
pub struct Vault {
    store: Arc<dyn BlobStore>,
    cipher: Box<dyn Cipher>,
    /// Held from read to write by both `save_journal_settings` and
    /// `merge_journal_settings` (see `settings.rs`), so a save and a
    /// concurrent merge cannot interleave their read and write and drop one
    /// side's change. It does not cover a plain sync pull, which the engine
    /// writes to the bare store beneath this vault.
    settings_lock: Mutex<()>,
    /// When this vault last took a snapshot of each entry — or, for an entry
    /// it has not snapshotted yet, when it first saw a save of it. A throttle
    /// hint only: lost on lock, never synced, and never read for anything but
    /// `snapshot_outgoing`'s periodic condition. Wrong in the safe direction —
    /// a snapshot another device took is invisible here, which can only cause
    /// one extra snapshot, never a missed gap snapshot.
    checkpoints: Mutex<HashMap<Id, DateTime<Utc>>>,
}

impl Vault {
    /// Opens the journal in `store`, unlocking its key with `passphrase` and
    /// creating a vault if the store is empty.
    ///
    /// The only place the create-or-open decision is made. Getting it wrong
    /// means writing a fresh key over a store whose entries were sealed under a
    /// different one, orphaning every entry in it. A store holding entries but
    /// no key is exactly that case, and is refused with
    /// [`KeyError::MissingKeyFile`] rather than treated as a first run.
    ///
    /// This decision lives here, rather than in `journal-key` where it began,
    /// because it needs both halves of the question: whether there is a key —
    /// that crate's business — and whether there are entries, which only this
    /// layer can answer.
    pub fn open(store: Arc<dyn BlobStore>, passphrase: &SecretString) -> Result<Self, VaultError> {
        let key = if Keyring::exists(&*store)? {
            Keyring::unlock(&*store, passphrase)?
        } else if holds_content(&*store)? {
            return Err(KeyError::MissingKeyFile.into());
        } else {
            Keyring::create(&*store, passphrase)?
        };
        Ok(Self::with_cipher(store, Box::new(key)))
    }

    /// Private so that the only cipher reachable from outside this crate is a
    /// real [`Keyring`]: the tests below stand a fake one up through here, and
    /// nothing else can.
    fn with_cipher(store: Arc<dyn BlobStore>, cipher: Box<dyn Cipher>) -> Self {
        Self {
            store,
            cipher,
            settings_lock: Mutex::new(()),
            checkpoints: Mutex::new(HashMap::new()),
        }
    }

    /// Lists all entry headers, most recent entry date first.
    ///
    /// By `entry_date`, not `updated_at`: the list is the journal's
    /// chronology, so editing an old entry must not move it to the top.
    ///
    /// Entries that fail to decrypt or parse are skipped and logged rather
    /// than failing the whole call.
    pub fn list_entries(&self) -> Result<Vec<Header>, VaultError> {
        let mut headers = Vec::new();
        self.read_each::<Header>(|header| headers.push(header))?;
        headers.sort_by_key(|header| std::cmp::Reverse(header.entry_date));
        Ok(headers)
    }

    /// Every entry whose title or body contains `query`, most recent entry
    /// date first — the order [`list_entries`](Self::list_entries) returns, so
    /// results read as the journal filtered rather than as a second ranking.
    ///
    /// Case-insensitive substring, and nothing more — no regex, no ranking,
    /// no stemming (`search.rs` holds the matching itself). The
    /// query is matched against the title and the markdown together, which is
    /// the whole point: `list_entries` returns headers without a body on
    /// purpose, so a search the frontend could run on what it already has
    /// could only ever match titles.
    ///
    /// **This decrypts the journal**, once per call, and holds one entry's
    /// plaintext at a time. There is deliberately no index, and the measured
    /// reason is that there is nothing to win: over 1,000 entries of ~1 kB a
    /// search costs ~100 ms in a release build — the same ~100 ms
    /// [`list_entries`](Self::list_entries) costs, since it decrypts every
    /// blob too, and the app already runs that on every unlock and after every
    /// sync pass. The ~1 s of scrypt is spent once, at unlock, on the key blob
    /// alone.
    ///
    /// A cached index would therefore save a cost the app already pays, in
    /// exchange for invalidating it on every save, every delete, every lock,
    /// and every entry sync pulls in behind the vault's back — through the
    /// store the engine writes *underneath* this layer, which cannot be
    /// observed from in here. Correct results are worth more than that, until
    /// there is a journal large enough to prove otherwise.
    ///
    /// Entries that fail to decrypt or parse are skipped, as in
    /// [`list_entries`](Self::list_entries): one bad blob must not fail a
    /// search any more than it fails a listing.
    pub fn search(&self, query: &str) -> Result<Vec<SearchMatch>, VaultError> {
        // An empty query matches nothing rather than everything: the caller
        // that wants everything has `list_entries`, and the search field is
        // empty far more often than it is not.
        let Some(query) = Query::new(query) else {
            return Ok(Vec::new());
        };

        let mut matches = Vec::new();
        self.read_each::<Entry>(|entry| {
            if query.matches(&entry.header.title) || query.matches(&entry.markdown) {
                matches.push(SearchMatch {
                    snippet: query.snippet(&entry.markdown),
                    header: entry.header,
                });
            }
        })?;
        matches.sort_by_key(|m| std::cmp::Reverse(m.header.entry_date));
        Ok(matches)
    }

    /// Hands `visit` every entry blob in the store, decoded, skipping the ones
    /// that will not read.
    ///
    /// The one place entries are enumerated, so that skipping a corrupt blob
    /// rather than failing the whole call is a property of the vault and not
    /// of each caller that remembered it. Generic over the decoded shape for
    /// the reason [`read`](Self::read) is: a listing wants headers and a
    /// search wants bodies, out of the same bytes.
    fn read_each<T: serde::de::DeserializeOwned>(
        &self,
        mut visit: impl FnMut(T),
    ) -> Result<(), VaultError> {
        for name in self.store.list()?.into_iter().filter(is_entry) {
            match self.read::<T>(&name) {
                Ok(Some(decoded)) => visit(decoded),
                // Listed a moment ago and gone now. Not an error — a delete
                // racing a list is legitimate — but worth a line, because the
                // alternative reading is a store losing blobs.
                Ok(None) => log::warn!("entry blob disappeared while listing: blob={name}"),
                Err(e) => {
                    // Rendered before the `warn!` rather than inside it: `age`'s
                    // error `Display` lazily loads its i18n bundle and logs while
                    // doing so, and a sink that holds a lock across formatting its
                    // own record (fern's file target, say) would deadlock on the
                    // reentrant call. Formatting here keeps those records outside.
                    let error = e.to_string();
                    log::warn!(
                        "skipping entry that failed to decrypt or parse: blob={name}, error={error}"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn get_entry(&self, id: &Id) -> Result<Option<Entry>, VaultError> {
        self.read(&blob_name(id))
    }

    /// Creates or overwrites the entry with the given id.
    ///
    /// Unconditional on purpose: the resolver calls this for the conflicted
    /// copy it writes and for a theirs-wins overwrite (ADR 0012, D1), and
    /// both of those must succeed regardless of what is on disk. Anything
    /// that should refuse a stale write wants [`save_entry_if`](Self::save_entry_if)
    /// instead.
    ///
    /// Now also reads whatever is currently on disk under this id — not for
    /// any precondition, `save_entry` still takes none — so it can decide
    /// whether that outgoing version is worth keeping as a snapshot before it
    /// is gone (see `snapshot_outgoing`).
    pub fn save_entry(&self, entry: &Entry) -> Result<(), VaultError> {
        if let Some(previous) = self.read::<Entry>(&blob_name(&entry.header.id))? {
            self.snapshot_outgoing(&previous, entry.header.updated_at)?;
        }
        self.write_entry(entry)
    }

    /// Saves `entry` only if the entry on disk is still the one the caller
    /// last saw: `expected` is the `updated_at` it was holding, or `None` for
    /// "does not exist yet". Refuses with [`VaultError::Stale`] otherwise,
    /// leaving whatever is on disk untouched.
    ///
    /// This is the precondition the open pane's autosave writes through
    /// (GEN-1): it is what lets the vault tell "I am overwriting my own
    /// earlier draft" from "I am about to overwrite an edit that arrived from
    /// somewhere else since I last read this entry".
    ///
    /// **Not atomic against the sync engine, and now less narrowly so.** The
    /// engine writes the bare `BlobStore` beneath this vault, from its own
    /// thread, so a pull that lands between the read below and the final
    /// write in [`write_entry`](Self::write_entry) is silently overwritten
    /// (see the plan's Non-goals). On a save that does not cross
    /// `snapshot::SNAPSHOT_MIN_AGE`, that window is still one read plus one
    /// encrypt plus one write. On a save that does, it now also contains a
    /// `store.list()` over every blob in the store and up to
    /// `snapshot::MAX_SNAPSHOTS_PER_ENTRY` deletes plus a snapshot `put`
    /// (`snapshot_outgoing`), so the window can be far larger than "a few
    /// milliseconds" and scales with total blob count. Closing it properly
    /// needs a lock or a compare-and-swap shared with the engine, which is
    /// its own design.
    pub fn save_entry_if(
        &self,
        entry: &Entry,
        expected: Option<DateTime<Utc>>,
    ) -> Result<(), VaultError> {
        let on_disk = self.read::<Entry>(&blob_name(&entry.header.id))?;
        if on_disk.as_ref().map(|e| e.header.updated_at) != expected {
            return Err(VaultError::Stale);
        }
        if let Some(previous) = &on_disk {
            self.snapshot_outgoing(previous, entry.header.updated_at)?;
        }
        self.write_entry(entry)
    }

    /// Headers of `id`'s past versions, most recent first. Empty, not an
    /// error, for an entry with no history yet.
    pub fn list_history(&self, id: &Id) -> Result<Vec<Header>, VaultError> {
        // `snapshot_names` is already newest-first, and a snapshot's name
        // encodes the exact `updated_at` its header holds, so pushing in that
        // order needs no re-sort.
        let names = self.snapshot_names(id)?;
        let mut headers = Vec::with_capacity(names.len());
        for (_, name) in names {
            if let Some(header) = self.read::<Header>(&name)? {
                headers.push(header);
            }
        }
        Ok(headers)
    }

    /// One past version's full content, or `None` if that exact snapshot is
    /// gone (already pruned, say).
    pub fn get_history_entry(
        &self,
        id: &Id,
        at: DateTime<Utc>,
    ) -> Result<Option<Entry>, VaultError> {
        self.read(&owned::name::<Snapshot>(id, &at))
    }

    /// Restores `id` to how it read at `at`: title and markdown come from the
    /// snapshot, `entry_date` is left as whatever the *live* entry currently
    /// has, and `updated_at` becomes now. `expected` is the `updated_at` the
    /// caller last saw for the live entry — the same conditional-write
    /// contract [`save_entry_if`](Self::save_entry_if) uses, so a restore
    /// started against a pane that's gone stale (another device edited
    /// meanwhile) is refused with [`VaultError::Stale`] rather than
    /// clobbering that edit.
    ///
    /// Snapshots the entry's current content first, unconditionally —
    /// bypassing [`snapshot_outgoing`](Self::snapshot_outgoing)'s throttle on
    /// purpose, not by calling it: a restore is exactly the kind of boundary
    /// worth always keeping, and it's what makes restoring itself undoable.
    pub fn restore_history(
        &self,
        id: &Id,
        at: DateTime<Utc>,
        expected: Option<DateTime<Utc>>,
    ) -> Result<Entry, VaultError> {
        // No live entry means there is nothing to restore *into* — never a
        // reason to write one: an orphaned snapshot, from a half-finished
        // delete or another device's edit racing this one's delete, must not
        // resurrect an entry the user deleted.
        let Some(live) = self.read::<Entry>(&blob_name(id))? else {
            return Err(VaultError::HistoryNotFound);
        };
        if Some(live.header.updated_at) != expected {
            return Err(VaultError::Stale);
        }
        let Some(snapshot) = self.read::<Entry>(&owned::name::<Snapshot>(id, &at))? else {
            return Err(VaultError::HistoryNotFound);
        };
        let now = Utc::now();
        let restored = Entry {
            header: Header {
                id: *id,
                title: snapshot.header.title,
                updated_at: now,
                entry_date: live.header.entry_date,
            },
            markdown: snapshot.markdown,
        };
        // Unconditional, unlike `snapshot_outgoing`: no age check on `live`.
        self.write_snapshot(&live, now)?;
        self.prune_history(id)?;
        self.write_entry(&restored)?;
        Ok(restored)
    }

    /// Serializes, encrypts and writes `entry` under its own id, with no
    /// precondition and no snapshot decision — the body every save ends in,
    /// once the caller above has already decided whether the version it is
    /// about to replace is worth keeping.
    fn write_entry(&self, entry: &Entry) -> Result<(), VaultError> {
        let plaintext = serde_json::to_vec(entry)?;
        let ciphertext = self.cipher.encrypt(&plaintext)?;
        self.store.put(&blob_name(&entry.header.id), &ciphertext)?;
        Ok(())
    }

    /// Decides whether `previous` — the version a save is about to replace —
    /// is worth keeping as a snapshot before it's gone, and takes one if so.
    ///
    /// Fires on either of two conditions, both compared against timestamps
    /// the caller supplies (never the wall clock), so tests can drive them
    /// with made-up times:
    ///
    /// - **Gap:** `previous` sat unchanged for at least
    ///   `snapshot::SNAPSHOT_MIN_AGE` — the start of a new editing session.
    /// - **Periodic:** this vault has not taken a snapshot of this entry for
    ///   at least `snapshot::SNAPSHOT_MIN_AGE` of continuous editing, so a long
    ///   session still produces a version from inside itself.
    fn snapshot_outgoing(&self, previous: &Entry, at: DateTime<Utc>) -> Result<(), VaultError> {
        let id = previous.header.id;
        let gap = at - previous.header.updated_at >= snapshot::SNAPSHOT_MIN_AGE;
        let periodic = {
            let mut checkpoints = self.checkpoints.lock().unwrap_or_else(|e| e.into_inner());
            // First save this session starts the clock; it isn't itself a
            // reason to snapshot (the gap condition covers a session's first
            // save).
            let since = *checkpoints.entry(id).or_insert(at);
            at - since >= snapshot::SNAPSHOT_MIN_AGE
        };
        if !(gap || periodic) {
            return Ok(());
        }
        self.write_snapshot(previous, at)?;
        self.prune_history(&id)
    }

    /// Writes `entry` as a snapshot and restarts the periodic clock for its
    /// id. `at` is the save that caused this snapshot, not `entry`'s own
    /// `updated_at` — the two agree for `snapshot_outgoing`'s callers, but
    /// keeping them distinct here is what lets `restore_history` restart the
    /// clock at `now` without renaming the snapshot it writes.
    fn write_snapshot(&self, entry: &Entry, at: DateTime<Utc>) -> Result<(), VaultError> {
        let name = owned::name::<Snapshot>(&entry.header.id, &entry.header.updated_at);
        let plaintext = serde_json::to_vec(entry)?;
        let ciphertext = self.cipher.encrypt(&plaintext)?;
        self.store.put(&name, &ciphertext)?;
        self.checkpoints
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(entry.header.id, at);
        Ok(())
    }

    /// `id`'s snapshot blobs, newest first, with the time parsed from each
    /// name.
    ///
    /// The one enumeration of an entry's history: `prune_history`,
    /// `list_history` and `delete_entry` all go through it, so there is
    /// exactly one definition of "this entry's snapshots".
    fn snapshot_names(&self, id: &Id) -> Result<Vec<(DateTime<Utc>, BlobName)>, VaultError> {
        let mut names = owned::owned_by::<Snapshot>(&*self.store, id)?;
        names.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        Ok(names)
    }

    /// Keeps the newest `snapshot::MAX_SNAPSHOTS_PER_ENTRY` snapshots for `id`,
    /// deleting the rest. Sorts by the timestamp embedded in the blob name
    /// (via `snapshot_names`), so this needs no decryption.
    fn prune_history(&self, id: &Id) -> Result<(), VaultError> {
        for (_, name) in self
            .snapshot_names(id)?
            .into_iter()
            .skip(snapshot::MAX_SNAPSHOTS_PER_ENTRY)
        {
            self.store.delete(&name)?;
        }
        Ok(())
    }

    /// Every media blob `id` owns. The one enumeration of an entry's media,
    /// like `snapshot_names` for snapshots.
    fn media_names(&self, id: &Id) -> Result<Vec<BlobName>, VaultError> {
        Ok(owned::owned_by::<MediaBlob>(&*self.store, id)?
            .into_iter()
            .map(|(_, name)| name)
            .collect())
    }

    /// Stores one variant of a media item under `entry`, encrypted. Immutable:
    /// an existing blob with the same ids is a retry (or an adoption copy) of
    /// the same bytes and counts as success.
    ///
    /// The bytes are the user's file for `Original`, so the declared format
    /// must match what they sniff as.
    pub fn save_media(
        &self,
        entry: &Id,
        media: &MediaId,
        variant: MediaVariant,
        meta: &MediaMeta,
        bytes: &[u8],
    ) -> Result<(), VaultError> {
        // No `_` arm: a new kind of media must decide its own validation.
        match meta {
            MediaMeta::Image {
                format,
                width,
                height,
            } => {
                if *width == 0 || *height == 0 {
                    return Err(VaultError::MediaType);
                }
                let displayable = match variant {
                    MediaVariant::Original => true,
                    MediaVariant::Display | MediaVariant::Thumb => format.is_web_safe(),
                };
                if !displayable || ImageFormat::sniff(bytes) != Some(*format) {
                    return Err(VaultError::MediaType);
                }
            }
        }
        if bytes.len() > media::max_bytes(variant) {
            return Err(VaultError::MediaTooLarge);
        }
        let ciphertext = self.cipher.encrypt(&media::encode_payload(meta, bytes))?;
        match self.store.put_new(
            &owned::name::<MediaBlob>(entry, &(*media, variant)),
            &ciphertext,
        ) {
            Ok(()) | Err(StoreError::AlreadyExists(_)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_media(
        &self,
        entry: &Id,
        media: &MediaId,
        variant: MediaVariant,
    ) -> Result<Option<Media>, VaultError> {
        let Some(ciphertext) = self
            .store
            .get(&owned::name::<MediaBlob>(entry, &(*media, variant)))?
        else {
            return Ok(None);
        };
        let plain = self.cipher.decrypt(&ciphertext)?;
        Ok(Some(media::decode_payload(&plain)?))
    }

    /// Makes `markdown` (the text of a conflicted copy, saved as `to`) own the
    /// media it references: each foreign `media:` reference whose original
    /// exists and is [`adoptable`](Self::adoptable) gets the ciphertext of every
    /// variant present copied under `to` (no decryption; the payload holds no
    /// ids) and its link rewritten. References to missing originals, and to a
    /// non-web-safe original whose thumbnail has not synced yet, are left
    /// alone. Idempotent, so safe
    /// under the sync resolver's at-least-once contract. Call it before saving
    /// the copy.
    pub fn adopt_media(&self, to: &Id, markdown: &str) -> Result<String, VaultError> {
        let mut out = markdown.to_owned();
        // Back to front, so earlier ranges stay valid while replacing.
        for r in media_refs(markdown).into_iter().rev() {
            if r.entry == *to {
                continue;
            }
            if !self.adoptable(&r.entry, &r.media)? {
                continue;
            }
            for variant in [
                MediaVariant::Original,
                MediaVariant::Display,
                MediaVariant::Thumb,
            ] {
                let target = owned::name::<MediaBlob>(to, &(r.media, variant));
                if self.store.contains(&target)? {
                    continue;
                }
                let Some(ciphertext) = self
                    .store
                    .get(&owned::name::<MediaBlob>(&r.entry, &(r.media, variant)))?
                else {
                    continue;
                };
                match self.store.put_new(&target, &ciphertext) {
                    Ok(()) | Err(StoreError::AlreadyExists(_)) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            out.replace_range(r.range, &format!("media:{to}/{}", r.media));
        }
        Ok(out)
    }

    /// Whether a copy may take over `entry`'s image `media` now. The original
    /// must be here. Then either its thumbnail is (a sync pass writes the
    /// display copy before the thumbnail, so the display copy is here too), or
    /// the original is web-safe, so the read view can show it as it is. A
    /// HEIC without its thumbnail would stay undecodable in the copy forever.
    fn adoptable(&self, entry: &Id, media: &MediaId) -> Result<bool, VaultError> {
        let name = |variant| owned::name::<MediaBlob>(entry, &(*media, variant));
        if !self.store.contains(&name(MediaVariant::Original))? {
            return Ok(false);
        }
        if self.store.contains(&name(MediaVariant::Thumb))? {
            return Ok(true);
        }
        Ok(
            match self.get_media(entry, media, MediaVariant::Original)? {
                Some(Media {
                    meta: MediaMeta::Image { format, .. },
                    ..
                }) => format.is_web_safe(),
                None => false,
            },
        )
    }

    /// Deletes `id`'s entry, its media and every snapshot it has.
    ///
    /// Media, then snapshots, entry blob last: a failure partway leaves the
    /// entry still visible and the user can retry. The other order would leave
    /// blobs that no UI can reach, and `holds_content` still counts them. An
    /// entry resurrected by sync ("edited here, deleted there") comes back
    /// without its media.
    pub fn delete_entry(&self, id: &Id) -> Result<(), VaultError> {
        for name in self.media_names(id)? {
            self.store.delete(&name)?;
        }
        for (_, name) in self.snapshot_names(id)? {
            self.store.delete(&name)?;
        }
        self.checkpoints
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        Ok(self.store.delete(&blob_name(id))?)
    }

    /// Decrypts and parses the bytes of an entry blob that is not in this
    /// vault's store — the other side of a sync conflict, say.
    pub fn decode_entry(&self, ciphertext: &[u8]) -> Result<Entry, VaultError> {
        let plaintext = self.cipher.decrypt(ciphertext)?;
        Ok(serde_json::from_slice(&plaintext)?)
    }

    /// Reads one blob and decodes it, or `None` if there is no such blob.
    ///
    /// Generic over the shape decoded because a full [`Entry`] and a bare
    /// [`Header`] come out of the same bytes — the header is flattened into the
    /// entry, so the smaller type parses the same JSON and skips the markdown.
    fn read<T: serde::de::DeserializeOwned>(
        &self,
        name: &BlobName,
    ) -> Result<Option<T>, VaultError> {
        let Some(ciphertext) = self.store.get(name)? else {
            return Ok(None);
        };
        let plaintext = self.cipher.decrypt(&ciphertext)?;
        Ok(Some(serde_json::from_slice(&plaintext)?))
    }
}

/// Whether `store` holds anything [`Vault::open`]'s orphan check must not
/// mint a fresh key over: an entry blob, a snapshot blob, a media blob, or the
/// settings blob.
///
/// A store holding only [`SETTINGS_BLOB`] and no key would otherwise pass as
/// an empty store and get a new key that can never read it — and the same is
/// true of a store holding only orphaned snapshots (their entry deleted, or
/// the key gone some other way).
fn holds_content(store: &dyn BlobStore) -> Result<bool, VaultError> {
    Ok(store.list()?.iter().any(|name| {
        is_entry(name)
            || settings::is_settings_blob(name)
            || snapshot::is_snapshot(name)
            || media::is_media(name)
    }))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use chrono::Utc;
    use journal_fs_store::FsStore;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A store in a directory no other test in this process will pick.
    pub(crate) fn temp_store() -> Arc<FsStore> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        Arc::new(FsStore::new(
            std::env::temp_dir().join(format!("journal-vault-test-{}-{n}", std::process::id())),
        ))
    }

    pub(crate) fn discard(store: &FsStore) {
        fs::remove_dir_all(store.dir()).ok();
    }

    pub(crate) fn passphrase(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    /// A stand-in for [`Keyring`] for the tests that are not about encryption.
    ///
    /// Reversible without a key, so these tests cost no scrypt: a `Vault` over
    /// a real keyring pays ~0.4s to open, for coverage of entry naming,
    /// listing and serde that has nothing to do with cryptography.
    pub(crate) struct FakeCipher;

    /// What makes `decrypt` fail for bytes this cipher did not write. Without a
    /// marker, XOR would happily "decrypt" a corrupt blob into garbage and the
    /// skip-corrupt tests below would prove nothing.
    const FAKE_MARKER: &[u8] = b"fake:";

    impl Cipher for FakeCipher {
        fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, KeyError> {
            let mut out = FAKE_MARKER.to_vec();
            out.extend(plaintext.iter().map(|b| b ^ 0x5A));
            Ok(out)
        }

        fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, KeyError> {
            let body = ciphertext
                .strip_prefix(FAKE_MARKER)
                .ok_or_else(KeyError::not_ciphertext)?;
            Ok(body.iter().map(|b| b ^ 0x5A).collect())
        }
    }

    /// A `Vault` whose entries are cheaply reversible rather than encrypted.
    pub(crate) fn fake_vault(store: Arc<FsStore>) -> Vault {
        Vault::with_cipher(store, Box::new(FakeCipher))
    }

    pub(crate) fn sample(id: Id) -> Entry {
        Entry {
            header: Header {
                id,
                title: "Test entry".to_string(),
                updated_at: Utc::now(),
                entry_date: Utc::now(),
            },
            markdown: "# hello".to_string(),
        }
    }

    #[test]
    fn save_get_list_delete_roundtrip() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        assert!(vault.list_entries().unwrap().is_empty());

        let a = Id::new();
        let b = Id::new();
        vault.save_entry(&sample(a)).unwrap();
        vault.save_entry(&sample(b)).unwrap();

        let fetched = vault.get_entry(&a).unwrap().unwrap();
        assert_eq!(fetched.markdown, "# hello");

        let headers = vault.list_entries().unwrap();
        assert_eq!(headers.len(), 2);
        assert!(headers.iter().all(|header| header.title == "Test entry"));

        vault.delete_entry(&a).unwrap();
        assert!(vault.get_entry(&a).unwrap().is_none());
        assert_eq!(vault.list_entries().unwrap().len(), 1);

        discard(&store);
    }

    /// The sync layer turns blob names back into entries with this, so it has
    /// to agree with the naming that produced them and refuse everything else.
    #[test]
    fn entry_id_inverts_the_blob_naming_and_refuses_other_names() {
        let id = Id::new();
        assert_eq!(entry_id(&blob_name(&id)), Some(id));

        let name = |s: &str| BlobName::try_from(s).unwrap();
        // The sealed key, and names from somewhere else.
        assert_eq!(entry_id(&name(".vault-key")), None);
        assert_eq!(entry_id(&name("notes.json")), None);
        assert_eq!(entry_id(&name("README")), None);
        // An id that parses but is not the spelling we write.
        let upper = format!("{}{ENTRY_EXT}", id.to_string().to_uppercase());
        assert_eq!(entry_id(&name(&upper)), None);
    }

    #[test]
    fn decode_entry_reads_the_bytes_a_save_wrote() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        vault.save_entry(&sample(id)).unwrap();

        let bytes = store.get(&blob_name(&id)).unwrap().unwrap();
        let entry = vault.decode_entry(&bytes).unwrap();
        assert_eq!(entry.header.id, id);
        assert_eq!(entry.markdown, "# hello");
        assert!(vault.decode_entry(b"not valid ciphertext").is_err());

        discard(&store);
    }

    #[test]
    fn get_missing_entry_returns_none() {
        let store = temp_store();
        assert!(fake_vault(store.clone())
            .get_entry(&Id::new())
            .unwrap()
            .is_none());
    }

    #[test]
    fn deleting_an_entry_twice_is_not_an_error() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        vault.save_entry(&sample(id)).unwrap();
        vault.delete_entry(&id).unwrap();
        vault.delete_entry(&id).unwrap();
        discard(&store);
    }

    #[test]
    fn saving_the_same_id_twice_overwrites_rather_than_duplicating() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        vault.save_entry(&sample(id)).unwrap();
        let mut edited = sample(id);
        edited.markdown = "# edited".to_string();
        vault.save_entry(&edited).unwrap();

        assert_eq!(vault.list_entries().unwrap().len(), 1);
        assert_eq!(vault.get_entry(&id).unwrap().unwrap().markdown, "# edited");
        discard(&store);
    }

    #[test]
    fn save_entry_if_writes_when_disk_matches() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        let entry = sample(id);
        vault.save_entry(&entry).unwrap();

        let mut edited = entry.clone();
        edited.markdown = "# edited".to_string();
        vault
            .save_entry_if(&edited, Some(entry.header.updated_at))
            .unwrap();

        assert_eq!(vault.get_entry(&id).unwrap().unwrap().markdown, "# edited");
        discard(&store);
    }

    #[test]
    fn save_entry_if_refuses_when_disk_moved_on() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        let v0 = sample(id);
        vault.save_entry(&v0).unwrap();

        let mut v1 = v0.clone();
        v1.header.updated_at = v0.header.updated_at + chrono::Duration::seconds(1);
        v1.markdown = "# v1".to_string();
        vault.save_entry(&v1).unwrap();

        let mut v2 = v1.clone();
        v2.header.updated_at = v1.header.updated_at + chrono::Duration::seconds(1);
        v2.markdown = "# v2".to_string();
        let Err(err) = vault.save_entry_if(&v2, Some(v0.header.updated_at)) else {
            panic!("save_entry_if wrote over a newer entry");
        };
        assert!(matches!(err, VaultError::Stale), "got {err:?}");

        assert_eq!(vault.get_entry(&id).unwrap().unwrap().markdown, "# v1");
        discard(&store);
    }

    #[test]
    fn save_entry_if_none_creates_but_refuses_over_an_existing_entry() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        let entry = sample(id);

        vault.save_entry_if(&entry, None).unwrap();
        assert_eq!(vault.get_entry(&id).unwrap().unwrap().markdown, "# hello");

        let mut edited = entry.clone();
        edited.markdown = "# edited".to_string();
        let Err(err) = vault.save_entry_if(&edited, None) else {
            panic!("save_entry_if(None) overwrote an entry that already existed");
        };
        assert!(matches!(err, VaultError::Stale), "got {err:?}");
        assert_eq!(vault.get_entry(&id).unwrap().unwrap().markdown, "# hello");
        discard(&store);
    }

    #[test]
    fn save_entry_if_some_refuses_when_the_entry_is_gone() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        let entry = sample(id);
        vault.save_entry(&entry).unwrap();
        vault.delete_entry(&id).unwrap();

        let Err(err) = vault.save_entry_if(&entry, Some(entry.header.updated_at)) else {
            panic!("save_entry_if resurrected a deleted entry");
        };
        assert!(matches!(err, VaultError::Stale), "got {err:?}");
        assert!(vault.get_entry(&id).unwrap().is_none());
        discard(&store);
    }

    /// The text is the contract: `src/utils/errors.ts`'s `isStaleEntry`
    /// compares an `Err(String)` against this exact string.
    #[test]
    fn stale_message_is_the_contract() {
        assert_eq!(
            VaultError::Stale.to_string(),
            "this entry was changed on another device since it was opened"
        );
    }

    #[test]
    fn list_entries_is_most_recent_entry_date_first() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        let old = Id::new();
        let new = Id::new();
        let mut older = sample(old);
        older.header.entry_date = Utc::now() - chrono::Duration::hours(1);
        older.header.title = "older".to_string();
        let mut newer = sample(new);
        newer.header.title = "newer".to_string();

        vault.save_entry(&older).unwrap();
        vault.save_entry(&newer).unwrap();

        let titles: Vec<_> = vault
            .list_entries()
            .unwrap()
            .into_iter()
            .map(|h| h.title)
            .collect();
        assert_eq!(titles, ["newer", "older"]);
        discard(&store);
    }

    /// The defect `entry_date` exists to fix: editing a months-old entry must
    /// not drag it to the top of the journal.
    #[test]
    fn list_entries_ignores_updated_at() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        let mut old = sample(Id::new());
        old.header.title = "old".to_string();
        old.header.entry_date = Utc::now() - chrono::Duration::days(90);
        // Written a moment ago — the typo fix — but about a day in June.
        old.header.updated_at = Utc::now();

        let mut recent = sample(Id::new());
        recent.header.title = "recent".to_string();
        recent.header.entry_date = Utc::now();
        recent.header.updated_at = Utc::now() - chrono::Duration::days(1);

        vault.save_entry(&old).unwrap();
        vault.save_entry(&recent).unwrap();

        let titles: Vec<_> = vault
            .list_entries()
            .unwrap()
            .into_iter()
            .map(|h| h.title)
            .collect();
        assert_eq!(titles, ["recent", "old"]);
        discard(&store);
    }

    /// Saving is a write, not a re-dating: the vault stores what it is given,
    /// and what it is given keeps its `entry_date`.
    #[test]
    fn saving_an_entry_leaves_its_entry_date_alone() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        let id = Id::new();
        let mut entry = sample(id);
        let entry_date = Utc::now() - chrono::Duration::days(30);
        entry.header.entry_date = entry_date;
        entry.header.updated_at = entry_date;
        vault.save_entry(&entry).unwrap();

        entry.markdown = "# edited".to_string();
        entry.header.updated_at = Utc::now();
        vault.save_entry(&entry).unwrap();

        let stored = vault.get_entry(&id).unwrap().unwrap();
        assert_eq!(stored.header.entry_date, entry_date);
        assert!(stored.header.updated_at > entry_date);
        discard(&store);
    }

    /// An entry whose text is `markdown`, dated `days_ago` so the ordering of
    /// several of them is fixed rather than a race between two `Utc::now()`s.
    fn entry(title: &str, markdown: &str, days_ago: i64) -> Entry {
        let mut entry = sample(Id::new());
        entry.header.title = title.to_string();
        entry.header.entry_date = Utc::now() - chrono::Duration::days(days_ago);
        entry.markdown = markdown.to_string();
        entry
    }

    fn titles(matches: &[SearchMatch]) -> Vec<&str> {
        matches.iter().map(|m| m.header.title.as_str()).collect()
    }

    /// The case a frontend-side search could never answer: `list_entries`
    /// returns headers without a body, so matching the text of an entry is the
    /// whole reason this lives in the backend.
    #[test]
    fn search_matches_the_body_of_an_entry_whose_title_does_not() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault
            .save_entry(&entry("Tuesday", "A long walk by the river.", 1))
            .unwrap();
        vault
            .save_entry(&entry("Wednesday", "Stayed in and read.", 0))
            .unwrap();

        let found = vault.search("river").unwrap();

        assert_eq!(titles(&found), ["Tuesday"]);
        assert!(found[0].snippet.contains("river"), "{found:?}");
        discard(&store);
    }

    #[test]
    fn search_matches_a_title_too_and_ignores_case() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault
            .save_entry(&entry("Coffee with Sam", "Nothing else to add.", 0))
            .unwrap();

        assert_eq!(
            titles(&vault.search("COFFEE").unwrap()),
            ["Coffee with Sam"]
        );
        assert_eq!(titles(&vault.search("sAm").unwrap()), ["Coffee with Sam"]);
        discard(&store);
    }

    /// An entry matched on both sides must appear once, not twice.
    #[test]
    fn an_entry_matching_in_both_places_is_one_result() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault
            .save_entry(&entry("Rain", "Rain all day.", 0))
            .unwrap();

        assert_eq!(vault.search("rain").unwrap().len(), 1);
        discard(&store);
    }

    /// Results are the journal filtered, in the order the list shows it.
    #[test]
    fn search_results_are_most_recent_entry_date_first() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.save_entry(&entry("older", "a walk", 10)).unwrap();
        vault.save_entry(&entry("newer", "a walk", 1)).unwrap();
        vault.save_entry(&entry("oldest", "a walk", 90)).unwrap();

        assert_eq!(
            titles(&vault.search("walk").unwrap()),
            ["newer", "older", "oldest"]
        );
        discard(&store);
    }

    /// The field is empty far more often than not, and the list already shows
    /// everything: an empty query is not a request for the whole journal.
    #[test]
    fn an_empty_query_matches_nothing() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault
            .save_entry(&entry("Something", "Anything.", 0))
            .unwrap();

        assert!(vault.search("").unwrap().is_empty());
        assert!(vault.search("   ").unwrap().is_empty());
        discard(&store);
    }

    /// The same skip-and-warn the listing does: one bad blob must not cost the
    /// user every result in the journal.
    #[test]
    fn search_skips_an_entry_that_will_not_decrypt() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.save_entry(&entry("Readable", "a walk", 0)).unwrap();
        store
            .put(&blob_name(&Id::new()), b"not valid ciphertext")
            .unwrap();

        assert_eq!(titles(&vault.search("walk").unwrap()), ["Readable"]);
        discard(&store);
    }

    /// The search reads what is on disk now, with no index in front of it: an
    /// entry saved a moment ago is findable, and a deleted one is not. The
    /// property the "no cache" decision in `Vault::search` buys.
    #[test]
    fn search_sees_the_latest_save_and_not_a_deleted_entry() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let mut written = entry("Draft", "the first thing I wrote", 0);
        vault.save_entry(&written).unwrap();
        assert_eq!(vault.search("first thing").unwrap().len(), 1);

        written.markdown = "rewritten entirely".to_string();
        vault.save_entry(&written).unwrap();
        assert!(vault.search("first thing").unwrap().is_empty());
        assert_eq!(vault.search("rewritten").unwrap().len(), 1);

        vault.delete_entry(&written.header.id).unwrap();
        assert!(vault.search("rewritten").unwrap().is_empty());
        discard(&store);
    }

    #[test]
    fn list_entries_skips_corrupt_blobs_without_failing() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        vault.save_entry(&sample(Id::new())).unwrap();
        store
            .put(&blob_name(&Id::new()), b"not valid ciphertext")
            .unwrap();

        let headers = vault.list_entries().unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].title, "Test entry");

        discard(&store);
    }

    /// P2-2: a single unreadable entry must not fail the whole listing.
    ///
    /// A *directory* named like an entry is the reachable case — a sync client
    /// or a stray `mkdir` is enough — and reading one yields an I/O error
    /// rather than the decrypt failure the test above covers.
    #[test]
    fn list_entries_survives_an_unreadable_entry() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        vault.save_entry(&sample(Id::new())).unwrap();
        fs::create_dir_all(store.dir().join(blob_name(&Id::new()).as_str())).unwrap();

        let headers = vault.list_entries().unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].title, "Test entry");

        discard(&store);
    }

    /// The other half of P2-2: a real file the process may not read.
    #[cfg(unix)]
    #[test]
    fn list_entries_survives_an_entry_with_no_read_permission() {
        use std::os::unix::fs::PermissionsExt;

        let store = temp_store();
        let vault = fake_vault(store.clone());

        vault.save_entry(&sample(Id::new())).unwrap();
        let unreadable = store.dir().join(blob_name(&Id::new()).as_str());
        fs::write(&unreadable, b"whatever").unwrap();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        // Running as root, the mode above is not enforced and the read would
        // succeed — which would pass this test for the wrong reason.
        if fs::read(&unreadable).is_ok() {
            discard(&store);
            return;
        }

        let headers = vault.list_entries().unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].title, "Test entry");

        discard(&store);
    }

    #[test]
    fn list_entries_ignores_blobs_that_are_not_entries() {
        // The vault's own key blob shares the store with the entries, so a
        // wrong answer here would try to decrypt the key as an entry and warn
        // about it on every launch.
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.save_entry(&sample(Id::new())).unwrap();
        store
            .put(
                &BlobName::literal(".vault-key"),
                b"sealed key, not an entry",
            )
            .unwrap();
        store
            .put(&BlobName::literal("notes.txt"), b"something else entirely")
            .unwrap();
        // RUST-3: this used to pass `is_entry`'s old `.ends_with(".json")`
        // check even though it is not a `<uuid>.json` name, and so counted
        // toward `holds_content` too.
        store
            .put(&BlobName::literal("notes.json"), b"also not an entry")
            .unwrap();

        assert_eq!(vault.list_entries().unwrap().len(), 1);
        discard(&store);
    }

    /// RUST-3: a store holding a non-uuid `.json` blob and nothing else must
    /// still open as an empty, fresh vault rather than tripping the
    /// [`Vault::open`] orphan check meant for a store that lost its key.
    #[test]
    fn a_store_holding_only_notes_json_opens_as_a_new_vault() {
        let store = temp_store();
        store
            .put(&BlobName::literal("notes.json"), b"not an entry")
            .unwrap();

        let vault = Vault::open(store.clone(), &passphrase("fresh start")).unwrap();
        assert!(vault.list_entries().unwrap().is_empty());
        assert!(Keyring::exists(&*store).unwrap());

        discard(&store);
    }

    #[test]
    fn a_store_that_was_never_written_lists_no_entries() {
        // No directory yet, and reading must not create one: `has_vault` asks
        // this question before the user has typed a passphrase.
        let store = temp_store();
        assert!(fake_vault(store.clone()).list_entries().unwrap().is_empty());
        assert!(!store.dir().exists());
    }

    #[test]
    fn open_creates_a_vault_then_reopens_it() {
        // The real thing, scrypt and all: two unlocks, proving an entry written
        // in one session is readable in the next.
        let store = temp_store();
        let id = Id::new();

        let first = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        first.save_entry(&sample(id)).unwrap();

        let second = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        assert_eq!(second.get_entry(&id).unwrap().unwrap().markdown, "# hello");

        discard(&store);
    }

    #[test]
    fn open_with_the_wrong_passphrase_does_not_open_the_journal() {
        let store = temp_store();
        Vault::open(store.clone(), &passphrase("hunter2")).unwrap();

        let Err(err) = Vault::open(store.clone(), &passphrase("typo")) else {
            panic!("the wrong passphrase opened the journal");
        };
        assert!(
            matches!(err, VaultError::Key(KeyError::WrongPassphrase)),
            "got {err:?}"
        );

        discard(&store);
    }

    #[test]
    fn open_refuses_when_the_key_is_missing_but_entries_remain() {
        // The case that must never be read as a first run: creating a fresh
        // vault here would seal a new key over entries encrypted to the old
        // one, and every entry would be unreadable forever.
        let store = temp_store();
        let vault = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        vault.save_entry(&sample(Id::new())).unwrap();
        store.delete(&BlobName::literal(".vault-key")).unwrap();

        let Err(err) = Vault::open(store.clone(), &passphrase("typo")) else {
            panic!("opened a journal whose key is gone");
        };
        assert!(
            matches!(err, VaultError::Key(KeyError::MissingKeyFile)),
            "got {err:?}"
        );
        // The assertion that matters: an error that still wrote a key would
        // have orphaned the entry anyway.
        assert!(!Keyring::exists(&*store).unwrap());

        discard(&store);
    }

    #[test]
    fn a_directory_named_like_an_entry_does_not_block_creating_a_vault() {
        let store = temp_store();
        fs::create_dir_all(store.dir().join(blob_name(&Id::new()).as_str())).unwrap();

        Vault::open(store.clone(), &passphrase("fresh start")).unwrap();
        assert!(Keyring::exists(&*store).unwrap());

        discard(&store);
    }

    /// P1-2: `src-tauri` renders a command's error with `to_string()` and the
    /// frontend shows that string verbatim, so these are user-facing copy and
    /// not developer notes. Two properties: the passphrase failure reads like
    /// copy, and `journal-key`'s own wording reaches the user unprefixed —
    /// wrapping it in "vault error: …" would put a developer's label in front
    /// of a sentence written for the user.
    #[test]
    fn error_messages_are_user_facing() {
        assert_eq!(
            VaultError::from(KeyError::WrongPassphrase).to_string(),
            "Incorrect passphrase."
        );

        for err in [KeyError::MissingKeyFile, KeyError::CorruptKeyFile] {
            let copy = err.to_string();
            assert!(
                copy.starts_with(|c: char| c.is_uppercase()),
                "{copy:?} is shown to the user as written"
            );
            assert_eq!(VaultError::from(err).to_string(), copy);
        }
    }

    /// P1-4 guard (green before and after — atomicity is `rename`'s guarantee,
    /// and racing threads can only ever fail to disprove it). Since P1-1 two
    /// saves for the same id can interleave, so the store writes to scratch and
    /// renames; this asserts the invariant that leaves behind, one layer up
    /// from where the rename happens.
    #[test]
    fn save_entry_leaves_no_temp_file() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        vault.save_entry(&sample(id)).unwrap();
        let mut second = sample(id);
        second.header.title = "Second".into();
        vault.save_entry(&second).unwrap();

        assert_eq!(
            vault.get_entry(&id).unwrap().unwrap().header.title,
            "Second"
        );
        assert_eq!(vault.list_entries().unwrap().len(), 1);
        assert!(
            !fs::read_dir(store.dir()).unwrap().any(|f| f
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")),
            "a scratch file was left in the vault directory"
        );

        discard(&store);
    }

    #[test]
    fn entries_are_encrypted_at_rest() {
        // The one test here that uses a real vault for the entries themselves:
        // everything above stands a fake cipher up in its place, so this is
        // what holds the property that the bytes on disk are unreadable.
        let store = temp_store();
        let vault = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        let id = Id::new();
        vault.save_entry(&sample(id)).unwrap();

        let on_disk = fs::read(store.dir().join(blob_name(&id).as_str())).unwrap();
        let as_text = String::from_utf8_lossy(&on_disk);
        assert!(!as_text.contains("Test entry"));
        assert!(!as_text.contains("hello"));

        discard(&store);
    }

    static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// `log`'s logger is global and install-once, so
    /// `list_entries_logs_the_blob_it_skipped` is the only test in this crate
    /// allowed to install one.
    struct CapturingLogger;

    impl log::Log for CapturingLogger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            // Formatted before the lock is taken: formatting a record can emit
            // records of its own, and `CAPTURED` is not reentrant.
            let message = record.args().to_string();
            CAPTURED.lock().unwrap().push(message);
        }

        fn flush(&self) {}
    }

    #[test]
    fn list_entries_logs_the_blob_it_skipped() {
        log::set_logger(&CapturingLogger).expect("only test installing a logger");
        log::set_max_level(log::LevelFilter::Trace);

        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.save_entry(&sample(Id::new())).unwrap();

        let corrupt = blob_name(&Id::new());
        store.put(&corrupt, b"not valid ciphertext").unwrap();

        assert_eq!(vault.list_entries().unwrap().len(), 1);

        let captured = CAPTURED.lock().unwrap().join("\n");
        assert!(
            captured.contains(corrupt.as_str()),
            "no log record named the skipped blob; captured: {captured:?}"
        );

        discard(&store);
    }

    fn snapshot_count(store: &FsStore) -> usize {
        store
            .list()
            .unwrap()
            .into_iter()
            .filter(|n| is_snapshot(n))
            .count()
    }

    #[test]
    fn save_entry_snapshots_the_outgoing_version_after_a_gap_but_not_before() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        let mut v0 = sample(id);
        v0.header.updated_at = Utc::now() - chrono::Duration::minutes(40);
        vault.save_entry(&v0).unwrap();
        assert_eq!(
            snapshot_count(&store),
            0,
            "the first save has nothing to snapshot"
        );

        // 5 minutes later: too soon to snapshot v0.
        let mut v1 = v0.clone();
        v1.header.updated_at = v0.header.updated_at + chrono::Duration::minutes(5);
        v1.markdown = "# v1".to_string();
        vault.save_entry(&v1).unwrap();
        assert_eq!(
            snapshot_count(&store),
            0,
            "a 5-minute gap must not snapshot"
        );

        // 30 minutes after that: old enough to snapshot v1.
        let mut v2 = v1.clone();
        v2.header.updated_at = v1.header.updated_at + chrono::Duration::minutes(30);
        v2.markdown = "# v2".to_string();
        vault.save_entry(&v2).unwrap();
        assert_eq!(
            snapshot_count(&store),
            1,
            "a 30-minute gap must snapshot the outgoing version"
        );

        discard(&store);
    }

    /// Saves one minute apart for 65 minutes, with made-up `updated_at`s so no
    /// gap ever reaches `SNAPSHOT_MIN_AGE`: the periodic condition alone must
    /// checkpoint twice, around the 30- and 60-minute marks.
    #[test]
    fn periodic_trigger_checkpoints_a_long_continuous_session() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        let start = Utc::now();

        let mut previous = sample(id);
        previous.header.updated_at = start;
        vault.save_entry(&previous).unwrap();
        assert_eq!(
            snapshot_count(&store),
            0,
            "the first save of the run takes none"
        );

        for minute in 1..=65 {
            let mut next = previous.clone();
            next.header.updated_at = start + chrono::Duration::minutes(minute);
            next.markdown = format!("# minute {minute}");
            vault.save_entry(&next).unwrap();
            previous = next;
        }

        assert_eq!(
            snapshot_count(&store),
            2,
            "a 65-minute session at 1-minute intervals should checkpoint twice"
        );

        discard(&store);
    }

    #[test]
    fn list_history_returns_newest_first_and_empty_for_no_history() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        assert!(vault.list_history(&id).unwrap().is_empty());

        let mut v0 = sample(id);
        v0.header.title = "v0".to_string();
        v0.header.updated_at = Utc::now() - chrono::Duration::minutes(90);
        vault.save_entry(&v0).unwrap();

        let mut v1 = v0.clone();
        v1.header.title = "v1".to_string();
        v1.header.updated_at = v0.header.updated_at + chrono::Duration::minutes(30);
        vault.save_entry(&v1).unwrap();

        let mut v2 = v1.clone();
        v2.header.title = "v2".to_string();
        v2.header.updated_at = v1.header.updated_at + chrono::Duration::minutes(30);
        vault.save_entry(&v2).unwrap();

        let titles: Vec<_> = vault
            .list_history(&id)
            .unwrap()
            .into_iter()
            .map(|h| h.title)
            .collect();
        assert_eq!(titles, ["v1", "v0"]);

        discard(&store);
    }

    #[test]
    fn get_history_entry_round_trips_and_returns_none_for_unknown_timestamp() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        let mut v0 = sample(id);
        v0.header.updated_at = Utc::now() - chrono::Duration::minutes(60);
        v0.markdown = "# v0 body".to_string();
        vault.save_entry(&v0).unwrap();

        let mut v1 = v0.clone();
        v1.header.updated_at = v0.header.updated_at + chrono::Duration::minutes(31);
        v1.markdown = "# v1 body".to_string();
        vault.save_entry(&v1).unwrap();

        let snapshot = vault
            .get_history_entry(&id, v0.header.updated_at)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.markdown, "# v0 body");

        assert!(vault.get_history_entry(&id, Utc::now()).unwrap().is_none());

        discard(&store);
    }

    #[test]
    fn writing_a_21st_old_enough_snapshot_prunes_the_oldest() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        let mut previous = sample(id);
        previous.header.updated_at = Utc::now() - chrono::Duration::minutes(30 * 22);
        previous.markdown = "# v0".to_string();
        vault.save_entry(&previous).unwrap();
        let v0_updated_at = previous.header.updated_at;

        for i in 1..=21 {
            let mut next = previous.clone();
            next.header.updated_at = previous.header.updated_at + chrono::Duration::minutes(30);
            next.markdown = format!("# v{i}");
            vault.save_entry(&next).unwrap();
            previous = next;
        }

        let history = vault.list_history(&id).unwrap();
        assert_eq!(history.len(), 20);
        assert!(
            vault
                .get_history_entry(&id, v0_updated_at)
                .unwrap()
                .is_none(),
            "the oldest snapshot should have been pruned"
        );
        let newest = vault
            .get_history_entry(&id, history[0].updated_at)
            .unwrap()
            .unwrap();
        assert_eq!(newest.markdown, "# v20");

        discard(&store);
    }

    #[test]
    fn restore_history_snapshots_current_content_regardless_of_recency_and_keeps_live_entry_date() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        let mut v0 = sample(id);
        v0.header.title = "Old title".to_string();
        v0.markdown = "# old body".to_string();
        v0.header.entry_date = Utc::now() - chrono::Duration::days(10);
        v0.header.updated_at = Utc::now() - chrono::Duration::minutes(60);
        vault.save_entry(&v0).unwrap();

        // A 31-minute gap: the ordinary throttle snapshots v0 automatically.
        let mut v1 = v0.clone();
        v1.header.title = "New title".to_string();
        v1.markdown = "# new body".to_string();
        v1.header.entry_date = Utc::now() - chrono::Duration::days(2);
        v1.header.updated_at = v0.header.updated_at + chrono::Duration::minutes(31);
        vault.save_entry(&v1).unwrap();
        assert_eq!(
            vault.list_history(&id).unwrap().len(),
            1,
            "v0 should already be snapshotted by the gap throttle"
        );

        // Restoring immediately after v1 — well under the throttle's
        // interval — still snapshots v1: the "always snapshot on restore"
        // rule, not the throttle.
        let restored = vault
            .restore_history(&id, v0.header.updated_at, Some(v1.header.updated_at))
            .unwrap();

        assert_eq!(restored.header.title, "Old title");
        assert_eq!(restored.markdown, "# old body");
        // entry_date comes from the *live* entry (v1's), not the restored
        // snapshot's (v0's).
        assert_eq!(restored.header.entry_date, v1.header.entry_date);
        assert!(restored.header.updated_at > v1.header.updated_at);
        assert_eq!(
            vault.get_entry(&id).unwrap().unwrap().markdown,
            "# old body"
        );

        let titles: Vec<_> = vault
            .list_history(&id)
            .unwrap()
            .into_iter()
            .map(|h| h.title)
            .collect();
        assert_eq!(
            titles.len(),
            2,
            "both v0 and the pre-restore v1 must be in history: {titles:?}"
        );
        assert!(titles.contains(&"Old title".to_string()));
        assert!(titles.contains(&"New title".to_string()));

        discard(&store);
    }

    #[test]
    fn restore_history_refuses_when_expected_does_not_match_the_live_entry() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        let mut v0 = sample(id);
        v0.header.updated_at = Utc::now() - chrono::Duration::minutes(60);
        vault.save_entry(&v0).unwrap();
        let mut v1 = v0.clone();
        v1.header.updated_at = v0.header.updated_at + chrono::Duration::minutes(31);
        vault.save_entry(&v1).unwrap();

        let Err(err) = vault.restore_history(&id, v0.header.updated_at, Some(Utc::now())) else {
            panic!("restore_history wrote over a live entry it hadn't seen");
        };
        assert!(matches!(err, VaultError::Stale), "got {err:?}");
        assert_eq!(
            vault.get_entry(&id).unwrap().unwrap().header.updated_at,
            v1.header.updated_at
        );

        discard(&store);
    }

    #[test]
    fn restore_history_and_get_history_entry_return_not_found_for_an_unknown_snapshot() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        let entry = sample(id);
        vault.save_entry(&entry).unwrap();

        assert!(vault.get_history_entry(&id, Utc::now()).unwrap().is_none());

        let Err(err) = vault.restore_history(&id, Utc::now(), Some(entry.header.updated_at)) else {
            panic!("restore_history restored a version that was never taken");
        };
        assert!(matches!(err, VaultError::HistoryNotFound), "got {err:?}");

        discard(&store);
    }

    /// Second review: an orphaned snapshot (its live entry gone, from a
    /// half-finished delete or a race with another device's delete) must
    /// never be restored into a fresh entry.
    #[test]
    fn restore_history_over_an_orphaned_snapshot_returns_not_found_and_writes_nothing() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        let orphan = sample(id);
        vault
            .write_snapshot(&orphan, orphan.header.updated_at)
            .unwrap();

        let Err(err) = vault.restore_history(&id, orphan.header.updated_at, None) else {
            panic!("restore_history resurrected a deleted entry from an orphaned snapshot");
        };
        assert!(matches!(err, VaultError::HistoryNotFound), "got {err:?}");
        assert!(vault.get_entry(&id).unwrap().is_none());

        discard(&store);
    }

    #[test]
    fn delete_entry_removes_its_snapshots_too() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        let mut v0 = sample(id);
        v0.header.updated_at = Utc::now() - chrono::Duration::minutes(60);
        vault.save_entry(&v0).unwrap();
        let mut v1 = v0.clone();
        v1.header.updated_at = v0.header.updated_at + chrono::Duration::minutes(31);
        vault.save_entry(&v1).unwrap();
        assert_eq!(vault.list_history(&id).unwrap().len(), 1);

        vault.delete_entry(&id).unwrap();

        assert!(vault.get_entry(&id).unwrap().is_none());
        assert!(vault.list_history(&id).unwrap().is_empty());
        assert_eq!(snapshot_count(&store), 0);

        discard(&store);
    }

    /// A store holding only a snapshot blob (its entry gone, its key gone
    /// some other way) must not read as an empty store either.
    #[test]
    fn open_refuses_when_the_key_is_missing_but_only_a_snapshot_remains() {
        let store = temp_store();
        let vault = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        let id = Id::new();
        let entry = sample(id);
        vault.save_entry(&entry).unwrap();
        vault
            .write_snapshot(&entry, entry.header.updated_at)
            .unwrap();
        // The live entry is gone, as if from a delete that removed only the
        // entry blob, but its snapshot is still in the store.
        store.delete(&blob_name(&id)).unwrap();
        store.delete(&BlobName::literal(".vault-key")).unwrap();

        let Err(err) = Vault::open(store.clone(), &passphrase("typo")) else {
            panic!("opened a fresh vault over a store still holding an orphaned snapshot");
        };
        assert!(
            matches!(err, VaultError::Key(KeyError::MissingKeyFile)),
            "got {err:?}"
        );

        discard(&store);
    }

    /// RUST-3-shaped: `list_entries`/`search` must keep ignoring snapshot
    /// blobs even though they share the `.json` extension with entries.
    #[test]
    fn list_entries_and_search_ignore_snapshot_blobs() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.save_entry(&entry("Readable", "a walk", 0)).unwrap();
        vault
            .write_snapshot(&sample(Id::new()), Utc::now())
            .unwrap();

        assert_eq!(vault.list_entries().unwrap().len(), 1);
        assert_eq!(vault.search("walk").unwrap().len(), 1);

        discard(&store);
    }

    fn img() -> MediaMeta {
        MediaMeta::Image {
            format: ImageFormat::Webp,
            width: 4,
            height: 3,
        }
    }

    /// Bytes that sniff as WebP, followed by `body`.
    fn webp(body: &[u8]) -> Vec<u8> {
        [b"RIFF\0\0\0\0WEBP".as_slice(), body].concat()
    }

    fn jpeg(body: &[u8]) -> Vec<u8> {
        [&[0xFF, 0xD8, 0xFF][..], body].concat()
    }

    fn heic() -> Vec<u8> {
        [&[0, 0, 0, 16][..], b"ftypheic", &[0; 4]].concat()
    }

    fn meta_of(format: ImageFormat) -> MediaMeta {
        MediaMeta::Image {
            format,
            width: 4,
            height: 3,
        }
    }

    fn media_count(store: &FsStore) -> usize {
        store.list().unwrap().iter().filter(|n| is_media(n)).count()
    }

    #[test]
    fn save_media_then_get_media_round_trips() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        vault
            .save_media(&e, &m, MediaVariant::Original, &img(), &webp(b"a\nbytes"))
            .unwrap();
        let got = vault
            .get_media(&e, &m, MediaVariant::Original)
            .unwrap()
            .unwrap();
        assert_eq!(got.meta, img());
        assert_eq!(got.bytes, webp(b"a\nbytes"));
        discard(&store);
    }

    #[test]
    fn save_media_twice_with_the_same_ids_is_ok() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        vault
            .save_media(&e, &m, MediaVariant::Original, &img(), &webp(b"x"))
            .unwrap();
        vault
            .save_media(&e, &m, MediaVariant::Original, &img(), &webp(b"x"))
            .unwrap();
        assert_eq!(media_count(&store), 1);
        discard(&store);
    }

    #[test]
    fn save_media_refuses_an_image_with_a_zero_dimension() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let bad = MediaMeta::Image {
            format: ImageFormat::Png,
            width: 0,
            height: 5,
        };
        let err = vault
            .save_media(
                &Id::new(),
                &MediaId::new(),
                MediaVariant::Original,
                &bad,
                &webp(b"x"),
            )
            .unwrap_err();
        assert!(matches!(err, VaultError::MediaType), "{err:?}");
        assert_eq!(media_count(&store), 0);
        discard(&store);
    }

    #[test]
    fn save_media_refuses_more_than_max_bytes() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let big = webp(&vec![0u8; media::MAX_ORIGINAL_BYTES]);
        let err = vault
            .save_media(
                &Id::new(),
                &MediaId::new(),
                MediaVariant::Original,
                &img(),
                &big,
            )
            .unwrap_err();
        assert!(matches!(err, VaultError::MediaTooLarge), "{err:?}");
        discard(&store);
    }

    #[test]
    fn get_media_returns_none_for_unknown_ids() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        assert!(vault
            .get_media(&Id::new(), &MediaId::new(), MediaVariant::Original)
            .unwrap()
            .is_none());
        discard(&store);
    }

    #[test]
    fn media_is_encrypted_at_rest() {
        let store = temp_store();
        let vault = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        let (e, m) = (Id::new(), MediaId::new());
        vault
            .save_media(
                &e,
                &m,
                MediaVariant::Original,
                &img(),
                &webp(b"SECRET-PIXELS-SECRET-PIXELS"),
            )
            .unwrap();
        let raw = store
            .get(&owned::name::<MediaBlob>(&e, &(m, MediaVariant::Original)))
            .unwrap()
            .unwrap();
        assert!(!raw.windows(6).any(|w| w == b"SECRET"));
        assert!(!raw.windows(5).any(|w| w == b"image"));
        assert_eq!(
            vault
                .get_media(&e, &m, MediaVariant::Original)
                .unwrap()
                .unwrap()
                .bytes,
            webp(b"SECRET-PIXELS-SECRET-PIXELS")
        );
        discard(&store);
    }

    #[test]
    fn delete_entry_removes_its_media_but_not_another_entrys() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (a, b) = (Id::new(), Id::new());
        vault.save_entry(&sample(a)).unwrap();
        vault.save_entry(&sample(b)).unwrap();
        let (ma, mb) = (MediaId::new(), MediaId::new());
        vault
            .save_media(&a, &ma, MediaVariant::Original, &img(), &webp(b"a"))
            .unwrap();
        vault
            .save_media(&b, &mb, MediaVariant::Original, &img(), &webp(b"b"))
            .unwrap();

        vault.delete_entry(&a).unwrap();

        assert!(vault
            .get_media(&a, &ma, MediaVariant::Original)
            .unwrap()
            .is_none());
        assert!(vault
            .get_media(&b, &mb, MediaVariant::Original)
            .unwrap()
            .is_some());
        discard(&store);
    }

    #[test]
    fn open_refuses_when_the_key_is_missing_but_only_media_remains() {
        let store = temp_store();
        let vault = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        vault
            .save_media(
                &Id::new(),
                &MediaId::new(),
                MediaVariant::Original,
                &img(),
                &webp(b"x"),
            )
            .unwrap();
        store.delete(&BlobName::literal(".vault-key")).unwrap();

        let Err(err) = Vault::open(store.clone(), &passphrase("typo")) else {
            panic!("opened a fresh vault over orphaned media");
        };
        assert!(
            matches!(err, VaultError::Key(KeyError::MissingKeyFile)),
            "got {err:?}"
        );
        discard(&store);
    }

    #[test]
    fn list_entries_and_search_ignore_media_blobs() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.save_entry(&entry("Readable", "a walk", 0)).unwrap();
        vault
            .save_media(
                &Id::new(),
                &MediaId::new(),
                MediaVariant::Original,
                &img(),
                &webp(b"walk"),
            )
            .unwrap();
        assert_eq!(vault.list_entries().unwrap().len(), 1);
        assert_eq!(vault.search("walk").unwrap().len(), 1);
        discard(&store);
    }

    #[test]
    fn adopt_media_copies_foreign_media_and_rewrites_the_links() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (from, to, m) = (Id::new(), Id::new(), MediaId::new());
        vault
            .save_media(&from, &m, MediaVariant::Original, &img(), &webp(b"pix"))
            .unwrap();
        let md = format!("hi ![x](media:{from}/{m}) there");

        let out = vault.adopt_media(&to, &md).unwrap();

        assert_eq!(out, format!("hi ![x](media:{to}/{m}) there"));
        assert_eq!(
            vault
                .get_media(&to, &m, MediaVariant::Original)
                .unwrap()
                .unwrap()
                .bytes,
            webp(b"pix")
        );
        assert!(vault
            .get_media(&from, &m, MediaVariant::Original)
            .unwrap()
            .is_some());
        discard(&store);
    }

    #[test]
    fn adopt_media_leaves_refs_to_missing_media_and_own_media_alone() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (to, own, gone) = (Id::new(), MediaId::new(), MediaId::new());
        vault
            .save_media(&to, &own, MediaVariant::Original, &img(), &webp(b"x"))
            .unwrap();
        let md = format!("![](media:{to}/{own}) ![](media:{}/{gone})", Id::new());
        assert_eq!(vault.adopt_media(&to, &md).unwrap(), md);
        assert_eq!(media_count(&store), 1);
        discard(&store);
    }

    #[test]
    fn adopt_media_is_idempotent() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (from, to, m) = (Id::new(), Id::new(), MediaId::new());
        vault
            .save_media(&from, &m, MediaVariant::Original, &img(), &webp(b"pix"))
            .unwrap();
        let md = format!("![](media:{from}/{m})");
        let once = vault.adopt_media(&to, &md).unwrap();
        // The original text again (a retry), and the rewritten text.
        assert_eq!(vault.adopt_media(&to, &md).unwrap(), once);
        assert_eq!(vault.adopt_media(&to, &once).unwrap(), once);
        assert_eq!(media_count(&store), 2);
        discard(&store);
    }

    #[test]
    fn adopted_media_survives_deleting_the_original_entry() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (from, to, m) = (Id::new(), Id::new(), MediaId::new());
        vault.save_entry(&sample(from)).unwrap();
        vault
            .save_media(&from, &m, MediaVariant::Original, &img(), &webp(b"pix"))
            .unwrap();
        vault
            .adopt_media(&to, &format!("![](media:{from}/{m})"))
            .unwrap();
        vault.delete_entry(&from).unwrap();
        assert_eq!(
            vault
                .get_media(&to, &m, MediaVariant::Original)
                .unwrap()
                .unwrap()
                .bytes,
            webp(b"pix")
        );
        discard(&store);
    }

    fn save(vault: &Vault, e: &Id, m: &MediaId, v: MediaVariant, meta: &MediaMeta, bytes: &[u8]) {
        vault.save_media(e, m, v, meta, bytes).unwrap();
    }

    #[test]
    fn save_media_then_get_media_round_trips_each_variant() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        save(
            &vault,
            &e,
            &m,
            MediaVariant::Original,
            &meta_of(ImageFormat::Heic),
            &heic(),
        );
        save(&vault, &e, &m, MediaVariant::Display, &img(), &webp(b"d"));
        save(&vault, &e, &m, MediaVariant::Thumb, &img(), &webp(b"t"));
        let bytes = |v| vault.get_media(&e, &m, v).unwrap().unwrap().bytes;
        assert_eq!(bytes(MediaVariant::Original), heic());
        assert_eq!(bytes(MediaVariant::Display), webp(b"d"));
        assert_eq!(bytes(MediaVariant::Thumb), webp(b"t"));
        assert_eq!(media_count(&store), 3);
        discard(&store);
    }

    #[test]
    fn get_media_is_none_for_a_missing_variant() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        save(&vault, &e, &m, MediaVariant::Original, &img(), &webp(b"o"));
        assert!(vault
            .get_media(&e, &m, MediaVariant::Thumb)
            .unwrap()
            .is_none());
        assert!(vault
            .get_media(&e, &m, MediaVariant::Display)
            .unwrap()
            .is_none());
        discard(&store);
    }

    #[test]
    fn save_media_refuses_bytes_that_do_not_match_the_format() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        for (format, bytes) in [
            (ImageFormat::Png, webp(b"x")),
            (ImageFormat::Webp, b"<svg/>".to_vec()),
            (ImageFormat::Jpeg, Vec::new()),
        ] {
            let err = vault
                .save_media(&e, &m, MediaVariant::Original, &meta_of(format), &bytes)
                .unwrap_err();
            assert!(matches!(err, VaultError::MediaType), "{format:?}: {err:?}");
        }
        assert_eq!(media_count(&store), 0);
        discard(&store);
    }

    #[test]
    fn save_media_refuses_a_non_web_safe_thumbnail_or_display_copy() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        for variant in [MediaVariant::Display, MediaVariant::Thumb] {
            let err = vault
                .save_media(&e, &m, variant, &meta_of(ImageFormat::Heic), &heic())
                .unwrap_err();
            assert!(matches!(err, VaultError::MediaType), "{variant:?}: {err:?}");
        }
        assert_eq!(media_count(&store), 0);
        discard(&store);
    }

    #[test]
    fn save_media_caps_each_variant() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        for (variant, cap) in [
            (MediaVariant::Original, media::MAX_ORIGINAL_BYTES),
            (MediaVariant::Display, media::MAX_DISPLAY_BYTES),
            (MediaVariant::Thumb, media::MAX_THUMB_BYTES),
        ] {
            let body = vec![0u8; cap];
            let err = vault
                .save_media(&e, &m, variant, &img(), &webp(&body))
                .unwrap_err();
            assert!(
                matches!(err, VaultError::MediaTooLarge),
                "{variant:?}: {err:?}"
            );
        }
        // Exactly at the cap is fine.
        let at_cap = webp(&vec![0u8; media::MAX_THUMB_BYTES - 12]);
        vault
            .save_media(&e, &m, MediaVariant::Thumb, &img(), &at_cap)
            .unwrap();
        discard(&store);
    }

    #[test]
    fn original_keeps_its_bytes_exactly() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        // A JPEG with an EXIF-looking APP1 segment, GPS text and a newline.
        let file = jpeg(b"\xE1\x00\x20Exif\0\0GPSLatitude\n=52.5\xFF\xD9");
        let meta = meta_of(ImageFormat::Jpeg);
        save(&vault, &e, &m, MediaVariant::Original, &meta, &file);
        let got = vault
            .get_media(&e, &m, MediaVariant::Original)
            .unwrap()
            .unwrap();
        assert_eq!(got.bytes, file);
        assert_eq!(got.meta, meta);
        discard(&store);
    }

    #[test]
    fn delete_entry_removes_every_variant() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (e, m) = (Id::new(), MediaId::new());
        vault.save_entry(&sample(e)).unwrap();
        for v in [
            MediaVariant::Original,
            MediaVariant::Display,
            MediaVariant::Thumb,
        ] {
            save(&vault, &e, &m, v, &img(), &webp(b"x"));
        }
        assert_eq!(media_count(&store), 3);
        vault.delete_entry(&e).unwrap();
        assert_eq!(media_count(&store), 0);
        discard(&store);
    }

    #[test]
    fn adopt_media_copies_every_variant_present() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (from, to, m) = (Id::new(), Id::new(), MediaId::new());
        save(
            &vault,
            &from,
            &m,
            MediaVariant::Original,
            &meta_of(ImageFormat::Heic),
            &heic(),
        );
        save(
            &vault,
            &from,
            &m,
            MediaVariant::Display,
            &img(),
            &webp(b"d"),
        );
        save(&vault, &from, &m, MediaVariant::Thumb, &img(), &webp(b"t"));

        vault
            .adopt_media(&to, &format!("![](media:{from}/{m})"))
            .unwrap();

        let bytes = |v| vault.get_media(&to, &m, v).unwrap().unwrap().bytes;
        assert_eq!(bytes(MediaVariant::Original), heic());
        assert_eq!(bytes(MediaVariant::Display), webp(b"d"));
        assert_eq!(bytes(MediaVariant::Thumb), webp(b"t"));
        discard(&store);
    }

    #[test]
    fn adopt_media_without_a_thumbnail_still_adopts() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (from, to, m) = (Id::new(), Id::new(), MediaId::new());
        save(
            &vault,
            &from,
            &m,
            MediaVariant::Original,
            &img(),
            &webp(b"o"),
        );
        let out = vault
            .adopt_media(&to, &format!("![](media:{from}/{m})"))
            .unwrap();
        assert_eq!(out, format!("![](media:{to}/{m})"));
        assert!(vault
            .get_media(&to, &m, MediaVariant::Original)
            .unwrap()
            .is_some());
        assert!(vault
            .get_media(&to, &m, MediaVariant::Thumb)
            .unwrap()
            .is_none());
        discard(&store);
    }

    #[test]
    fn adopt_media_leaves_a_heic_original_without_its_thumbnail() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (from, to, m) = (Id::new(), Id::new(), MediaId::new());
        save(
            &vault,
            &from,
            &m,
            MediaVariant::Original,
            &meta_of(ImageFormat::Heic),
            &heic(),
        );
        save(
            &vault,
            &from,
            &m,
            MediaVariant::Display,
            &img(),
            &webp(b"d"),
        );
        let md = format!("![](media:{from}/{m})");
        assert_eq!(vault.adopt_media(&to, &md).unwrap(), md);
        assert_eq!(media_count(&store), 2);
        discard(&store);
    }

    #[test]
    fn adopt_media_leaves_the_ref_when_only_a_thumbnail_exists() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (from, to, m) = (Id::new(), Id::new(), MediaId::new());
        save(&vault, &from, &m, MediaVariant::Thumb, &img(), &webp(b"t"));
        let md = format!("![](media:{from}/{m})");
        assert_eq!(vault.adopt_media(&to, &md).unwrap(), md);
        assert_eq!(media_count(&store), 1);
        discard(&store);
    }

    #[test]
    fn open_refuses_when_only_a_thumbnail_remains() {
        let store = temp_store();
        let vault = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        save(
            &vault,
            &Id::new(),
            &MediaId::new(),
            MediaVariant::Thumb,
            &img(),
            &webp(b"t"),
        );
        store.delete(&BlobName::literal(".vault-key")).unwrap();
        let Err(err) = Vault::open(store.clone(), &passphrase("typo")) else {
            panic!("opened a fresh vault over an orphaned thumbnail");
        };
        assert!(
            matches!(err, VaultError::Key(KeyError::MissingKeyFile)),
            "got {err:?}"
        );
        discard(&store);
    }
}
