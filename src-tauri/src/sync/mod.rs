//! Cloud sync, wired into the app: the worker thread, the connect flow, the
//! status the frontend sees, and the commands that drive them.
//!
//! The crates below do the work — `journal-sync-engine` reconciles blobs, an adapter
//! such as `journal-dropbox` speaks to one provider — and know nothing of
//! Tauri. This module
//! is the glue, and holds the parts that are the app's own decisions: when to
//! sync ([`worker`]), what a conflict means ([`resolver`]), how the browser
//! gets back to the app ([`oauth`]) and what the UI is told ([`status`]).
//!
//! Sync runs while the vault is locked. The engine holds no key and moves
//! ciphertext, so logging out stops nothing and needs no "unsynced changes"
//! warning; only conflict resolution needs the key, and waits for it.
//!
//! Nothing here holds the vault's lock state across I/O: the resolver is given
//! a handle it owns (see [`resolver::VaultHandle`]).

pub mod dropbox;
pub mod oauth;
pub mod resolver;
pub mod status;
pub mod worker;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use journal_api::{AdoptedJournal, ConnectOutcome, ProviderInfo, SyncStatusPayload};
use journal_credential::{Credential, CredentialStore, RefreshToken};
use journal_key::Keyring;
use journal_remote::RemoteStore;
use journal_store::{BlobName, BlobStore};
use journal_sync_engine::{erase_state, probe_key, KeyProbe, SyncEngine, SyncStatus};
use journal_vault::is_media;
use tauri::State;
use url::Url;

pub use dropbox::{Dropbox, KeySource};
use resolver::{VaultHandle, VaultResolver};
use status::Coalescer;
use worker::{Timing, Worker};

/// Providers the app lists as coming later. They have no adapter, so
/// `connect_remote` refuses them as unknown.
const PLANNED_PROVIDERS: [(&str, &str); 3] = [
    ("google_drive", "Google Drive"),
    ("icloud", "iCloud"),
    ("webdav", "WebDAV"),
];

/// How long the user has to finish in the browser.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The most often `sync://status` is sent.
const STATUS_INTERVAL: Duration = Duration::from_millis(250);

/// Where the app tells the frontend things.
///
/// A trait so the tests can record what was sent without a webview.
pub trait Events: Send + Sync {
    /// `sync://status`: the latest status. Already throttled.
    fn status(&self, payload: &SyncStatusPayload);
    /// `sync://changed`: a pass changed entries on disk, so the list is stale.
    fn changed(&self);
}

/// Sends the events over Tauri's own channel.
pub struct TauriEvents<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> Events for TauriEvents<R> {
    fn status(&self, payload: &SyncStatusPayload) {
        use tauri::Emitter;
        if let Err(e) = self.0.emit("sync://status", payload) {
            log::warn!("could not send the sync status: {e}");
        }
    }

    fn changed(&self) {
        use tauri::Emitter;
        if let Err(e) = self.0.emit("sync://changed", ()) {
            log::warn!("could not announce a sync change: {e}");
        }
    }
}

/// Redeems an authorization code for the credential. Blocks on the network.
pub type Redeem = Box<dyn FnOnce(&str) -> Result<RefreshToken, String> + Send>;

/// Shows the user a page: the provider's, in their browser.
pub type OpenUrl = Arc<dyn Fn(&Url) -> Result<(), String> + Send + Sync>;

/// A connection that has been started: where to send the user, and how to turn
/// what comes back into a credential.
pub struct Connection {
    /// The page the user approves the app on.
    pub url: Url,
    /// What the redirect must echo back as `state`.
    pub state: String,
    /// The app the grant will be for, when the provider has one per build or
    /// per user. Saved with the login; every later use of the token goes
    /// through this app, never the one configured now.
    pub app: Option<String>,
    pub redeem: Redeem,
}

/// What the app needs of a cloud provider beyond the `RemoteStore` itself: how
/// to connect an account, and how to reach it afterwards. A build offers a list
/// of these ([`SyncConfig::providers`]); the tests stand in one that needs no
/// network.
pub trait Provider: Send + Sync {
    /// Names this provider in the saved login and in the `connect_remote`
    /// command. Lowercase, and never changes once shipped: a saved login is
    /// matched to its provider by it.
    fn id(&self) -> &'static str;

    /// What the user calls it, for the words the app says about it.
    fn name(&self) -> &'static str;

    /// The loopback port the redirect must come back to, for a provider that
    /// matches it against the URIs registered with the app (Dropbox). `None`,
    /// the default, takes any free port, which is what a provider that
    /// accepts any loopback port should do.
    fn redirect_port(&self) -> Option<u16> {
        None
    }

    /// Whether connecting can start now. A provider that needs a setting the
    /// user has not made yet (Dropbox's app key) says `false`.
    fn configured(&self) -> bool {
        true
    }

    /// Starts connecting. `redirect_uri` is where the provider will send the
    /// browser back to.
    fn begin(&self, redirect_uri: &str) -> Result<Connection, String>;

    /// The provider's stable id for the account `token` acts as. Called once
    /// per connection, after the code is redeemed, and saved with the login:
    /// sync state describes one account's remote, so a different account of
    /// the same provider must start from a clean state. Blocks on the network.
    ///
    /// `app` is the [`Connection::app`] the token was granted under.
    fn account(&self, app: Option<&str>, token: &RefreshToken) -> Result<String, String>;

    /// The remote for an account that has been connected. `app` is the one
    /// saved with the login.
    ///
    /// `install` calls this *before* taking the `lifecycle` lock, so it must
    /// not block on the network, and may read settings without inverting the
    /// lock order (see `settings.rs`).
    fn remote(
        &self,
        app: Option<&str>,
        token: &RefreshToken,
    ) -> Result<Arc<dyn RemoteStore>, String>;

    /// Revokes `token` at the provider, so a login this app no longer uses
    /// stops working there too (SEC-2).
    ///
    /// Best effort: it blocks on the network, and a failure is only logged by
    /// the caller, never surfaced to the user — the local disconnect or
    /// reconnect has already succeeded by the time this runs. The default does
    /// nothing, for a provider with no revoke endpoint.
    fn revoke(&self, app: Option<&str>, token: &RefreshToken) -> Result<(), String> {
        let _ = (app, token);
        Ok(())
    }
}

/// The running worker, present exactly while a provider is connected.
///
/// Created before the vault's store, because that store's change callback
/// reaches the worker through it (see [`Slot::dirty`]), and connecting can
/// happen long after the store exists.
#[derive(Default)]
pub struct Slot(Mutex<Option<Arc<Worker>>>);

impl Slot {
    fn get(&self) -> Option<Arc<Worker>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, worker: Arc<Worker>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(worker);
    }

    fn take(&self) -> Option<Arc<Worker>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// A blob was just written or deleted through the vault's store. A no-op
    /// when nothing is connected: connecting scans the store against the sync
    /// state, so a change made before then is found anyway.
    ///
    /// Runs on the caller's thread and holds the slot only to clone the
    /// handle, so a save never waits on a sync.
    pub fn dirty(&self, name: &BlobName) {
        if let Some(worker) = self.get() {
            worker.saved(name);
        }
    }

    fn payload(&self) -> SyncStatusPayload {
        match self.get() {
            Some(worker) => status::payload(
                Some(worker.provider()),
                &worker.engine().status(),
                worker.detail().as_deref(),
            ),
            None => status::payload(None, &SyncStatus::default(), None),
        }
    }
}

/// Everything [`SyncControl`] is made of.
pub struct SyncConfig {
    pub slot: Arc<Slot>,
    /// The store beneath the vault's, which the engine syncs. Not the one that
    /// notifies: the engine's own writes are not edits.
    pub local: Arc<dyn BlobStore>,
    pub vault: VaultHandle,
    pub events: Arc<dyn Events>,
    /// The providers this build can connect to. Ids are unique.
    pub providers: Vec<Arc<dyn Provider>>,
    /// The one saved login, and which provider it is for.
    pub credentials: Arc<dyn CredentialStore>,
    /// The directory `local` lives in, which switching journals moves aside.
    pub journal_dir: PathBuf,
    /// The engine's per-device state. Outside the store, so it is never synced.
    pub state_path: PathBuf,
    /// Shows the user the provider's page.
    pub open_url: OpenUrl,
    pub timing: Timing,
}

/// Puts back a vault that [`SyncControl::adopt_remote`] locked.
pub type VaultUndo = Box<dyn FnOnce() + Send>;

/// Where a login being installed came from.
#[derive(Clone, Copy)]
enum Login {
    /// Read back from the store, in an earlier run.
    Saved,
    /// Just granted, and not yet saved.
    New,
}

/// The app's sync, managed by Tauri as an `Arc` so a command can hand it to the
/// blocking pool.
pub struct SyncControl {
    config: SyncConfig,
    coalescer: Coalescer,
    /// Connecting, reconnecting and disconnecting take turns. Not held while
    /// waiting for the browser, which can take minutes.
    lifecycle: Mutex<()>,
    /// The `Timing` a worker started from here uses, kept current by
    /// `set_interval` for workers started later (on connect or resume) and
    /// read by `install` under the same `lifecycle` lock a running worker's
    /// update is sent under (see `set_interval`'s doc comment).
    timing: Mutex<Timing>,
    /// A login that was granted but found the cloud holding a different
    /// journal, waiting for the user to choose. Memory only: it is never
    /// saved, so a restart forgets it and the old login keeps working.
    pending: Mutex<Option<Credential>>,
    /// Test-only: how many times `set_interval` was called, so a test can
    /// check `settings::DeviceSettingsState::update` skips the call when the
    /// interval did not change.
    #[cfg(test)]
    set_interval_calls: std::sync::atomic::AtomicUsize,
}

impl SyncControl {
    pub fn new(config: SyncConfig) -> std::io::Result<Self> {
        let coalescer = {
            let slot = config.slot.clone();
            Coalescer::new(
                STATUS_INTERVAL,
                move || slot.payload(),
                config.events.clone(),
            )?
        };
        let timing = Mutex::new(config.timing);
        Ok(Self {
            config,
            coalescer,
            lifecycle: Mutex::new(()),
            timing,
            pending: Mutex::new(None),
            #[cfg(test)]
            set_interval_calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// The device settings' `sync_interval_minutes` changed. Stores the value
    /// for a worker started later, and forwards it to one already running.
    ///
    /// Holds `turn()` (the `lifecycle` lock) for both steps: without it, a
    /// change made while `resume` is starting a worker at launch could be
    /// stored after `install` read the old value but sent before the worker
    /// is in the slot, so no one would get it until the next restart.
    /// `settings::DeviceSettingsState::update` only calls this when the
    /// interval actually changed, so a slow `connect`/`disconnect` holding
    /// this same lock is not on the path of every unrelated settings change.
    pub fn set_interval(&self, interval: Option<Duration>) {
        #[cfg(test)]
        self.set_interval_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _turn = self.turn();
        self.timing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .interval = interval;
        if let Some(worker) = self.config.slot.get() {
            worker.set_interval(interval);
        }
    }

    /// Test-only: how many times `set_interval` has been called.
    #[cfg(test)]
    pub(crate) fn set_interval_call_count_for_test(&self) -> usize {
        self.set_interval_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Test-only: the `Timing` `install` would hand a freshly started worker
    /// right now.
    #[cfg(test)]
    pub(crate) fn current_timing_for_test(&self) -> Timing {
        *self.timing.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Test-only: the `Timing` the currently running worker, if any, was
    /// started with.
    #[cfg(test)]
    pub(crate) fn running_worker_timing_for_test(&self) -> Option<Timing> {
        self.config.slot.get().map(|worker| worker.started_with())
    }

    /// The provider with this id, if the build offers it.
    fn provider(&self, id: &str) -> Result<&dyn Provider, String> {
        self.config
            .providers
            .iter()
            .find(|p| p.id() == id)
            .map(AsRef::as_ref)
            .ok_or_else(|| format!("unknown sync provider {id:?}"))
    }

    /// Revokes `credential`'s token at its provider, on a background thread
    /// (SEC-2): the local change (a disconnect or a reconnect) has already
    /// succeeded by the time this is called, and revoking is best effort, so
    /// nothing here can be allowed to hold that up on the network. A failure to
    /// revoke, or to find the provider or start the thread, is only logged —
    /// never the token itself.
    fn revoke_later(&self, credential: Credential) {
        let Some(provider) = self
            .config
            .providers
            .iter()
            .find(|p| p.id() == credential.provider)
            .cloned()
        else {
            return;
        };
        let name = provider.name();
        let spawned = thread::Builder::new()
            .name("sync-revoke".to_owned())
            .spawn(move || {
                if let Err(e) = provider.revoke(credential.app.as_deref(), &credential.token) {
                    log::warn!("could not revoke the old {} login: {e}", provider.name());
                }
            });
        if let Err(e) = spawned {
            log::warn!("could not start a thread to revoke the old {name} login: {e}");
        }
    }

    /// Starts syncing if an account was connected in an earlier run. Scans the
    /// whole local store, so call it off the main thread.
    pub fn resume(&self) {
        match self.config.credentials.load() {
            Ok(Some(credential)) => {
                let resumed = self
                    .provider(&credential.provider)
                    .and_then(|provider| self.install(provider, &credential, Login::Saved));
                if let Err(e) = resumed {
                    log::warn!("could not resume sync: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => log::warn!("could not read the saved login, sync stays off: {e}"),
        }
    }

    pub fn payload(&self) -> SyncStatusPayload {
        self.config.slot.payload()
    }

    /// The providers a user can connect to, in the order the build lists them.
    pub fn providers(&self) -> Vec<ProviderInfo> {
        self.config
            .providers
            .iter()
            .map(|p| ProviderInfo {
                id: p.id().to_owned(),
                name: p.name().to_owned(),
                available: true,
                configured: p.configured(),
            })
            .chain(
                PLANNED_PROVIDERS
                    .iter()
                    .filter(|(id, _)| self.config.providers.iter().all(|p| p.id() != *id))
                    .map(|(id, name)| ProviderInfo {
                        id: (*id).to_owned(),
                        name: (*name).to_owned(),
                        available: false,
                        configured: false,
                    }),
            )
            .collect()
    }

    /// Sync soon: the vault was just unlocked, so a conflict that had to wait
    /// for the key can be settled.
    pub fn kick(&self) {
        if let Some(worker) = self.config.slot.get() {
            worker.soon();
        }
    }

    pub fn sync_now(&self) -> Result<(), String> {
        self.config
            .slot
            .get()
            .ok_or_else(|| "sync is not connected".to_owned())?
            .sync_now()
    }

    /// Connects an account: sends the user to the provider, waits for the
    /// browser to come back, saves the login and starts syncing.
    ///
    /// Works before there is a vault: on a second device this is how the
    /// journal's key arrives. It runs one pass before returning, so that the
    /// caller can ask `has_vault` again and find it. That pass failing does not
    /// fail the connection — the login is good — and the status says why.
    ///
    /// Connecting to a different provider than the one connected replaces it.
    /// The old login is only replaced once the new one has been granted, so a
    /// browser tab closed halfway leaves what was working still working.
    ///
    /// Before anything is saved, the cloud's key is compared with this
    /// device's ([`probe_key`]). If they are two different journals the login
    /// is kept in memory only and the outcome is
    /// [`ConnectOutcome::DifferentJournal`]: nothing changed, so a login that
    /// was working still works, and the user chooses between
    /// [`discard_pending_connection`](Self::discard_pending_connection) and
    /// [`adopt_remote`](Self::adopt_remote).
    pub fn connect(&self, provider_id: &str) -> Result<ConnectOutcome, String> {
        let cfg = &self.config;
        self.discard_pending();
        let provider = self.provider(provider_id)?;
        let port = provider.redirect_port().unwrap_or(0);
        let (listener, redirect_uri) = oauth::bind(port, provider.name())?;
        let Connection {
            url,
            state,
            app,
            redeem,
        } = provider.begin(&redirect_uri)?;
        (cfg.open_url)(&url).map_err(|e| format!("could not open the browser: {e}"))?;
        let code = oauth::await_code(&listener, &state, LOGIN_TIMEOUT, provider.name())?;
        drop(listener);
        let token = redeem(&code)?;
        let account = provider.account(app.as_deref(), &token)?;
        let mut credential = Credential::new(provider.id(), token)
            .try_with_account(account)
            .ok_or_else(|| format!("{} returned an unusable account id", provider.name()))?;
        if let Some(app) = app {
            credential = credential
                .try_with_app(app)
                .ok_or_else(|| format!("{} returned an unusable app id", provider.name()))?;
        }

        let remote = provider.remote(credential.app.as_deref(), &credential.token)?;
        let probe = probe_key(&*cfg.local, &*remote).map_err(|e| e.to_string())?;
        if probe == KeyProbe::Different {
            log::info!("connect found a different journal; waiting for the user");
            *self.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some(credential);
            return Ok(ConnectOutcome::DifferentJournal);
        }

        // Read before `install` so a failed install (the old login may still
        // be the one in use) revokes nothing.
        let old = cfg.credentials.load().ok().flatten();
        let worker = self.install_with(provider, &credential, Login::New, remote)?;
        // A new grant leaves the old token valid at the provider even when it
        // is the same account, so compare tokens, not accounts.
        if let Some(old) = old {
            if old.token.expose() != credential.token.expose() {
                self.revoke_later(old);
            }
        }
        if let Err(e) = worker.sync_now() {
            log::warn!("the first sync after connecting failed: {e}");
        }
        Ok(ConnectOutcome::Connected)
    }

    fn take_pending(&self) -> Option<Credential> {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    fn restore_pending(&self, credential: Credential) {
        *self.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some(credential);
    }

    /// Drops the pending login and revokes its token: it was granted and never
    /// used.
    fn discard_pending(&self) {
        if let Some(credential) = self.take_pending() {
            self.revoke_later(credential);
        }
    }

    /// The user chose "cancel" on a different journal.
    pub fn discard_pending_connection(&self) -> Result<(), String> {
        self.discard_pending();
        Ok(())
    }

    /// Whether the connected cloud holds a journal (a key). Answers `true` for
    /// anything but an empty cloud, so the login screen never offers to create
    /// a journal while one is on its way. Takes no lock and writes nothing.
    pub fn cloud_has_journal(&self) -> Result<bool, String> {
        let cfg = &self.config;
        let credential = cfg
            .credentials
            .load()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| NOTHING_CONNECTED.to_owned())?;
        let provider = self.provider(&credential.provider)?;
        let remote = provider.remote(credential.app.as_deref(), &credential.token)?;
        let probe = probe_key(&*cfg.local, &*remote).map_err(|e| e.to_string())?;
        Ok(probe != KeyProbe::NoRemoteKey)
    }

    /// Switches this device to the cloud's journal: this device's journal is
    /// moved aside (never deleted), and the first pass downloads the cloud's.
    ///
    /// `Err` means nothing changed: the journal is in place, the saved login
    /// and the engine state are as they were, the vault is unlocked again and
    /// a pending login is still pending. `Ok` means the journal was moved and
    /// the vault is locked, even when a later step failed (`ready: false`).
    ///
    /// The order is chosen so that a failure, or a crash, after any step
    /// leaves something the next launch handles safely: the state is erased
    /// *before* the journal moves (state next to an empty journal reads as
    /// every blob deleted, and the pass would delete them from the cloud), and
    /// the new login is saved before the move (a new login over the old
    /// journal halts on the key mismatch before touching anything).
    pub fn adopt_remote(
        &self,
        lock_vault: &dyn Fn() -> Result<VaultUndo, String>,
    ) -> Result<AdoptedJournal, String> {
        let cfg = &self.config;
        let pending = self.take_pending();
        let result = self.adopt_inner(pending.as_ref(), lock_vault);
        if let (Err(_), Some(credential)) = (&result, pending) {
            self.restore_pending(credential);
        }
        let (worker, backup_dir) = result?;
        // After `turn()` is released: the first pass downloads everything.
        if let Some(worker) = worker {
            if let Err(e) = worker.sync_now() {
                log::warn!("the first sync after switching journals failed: {e}");
            }
        }
        let ready = Keyring::exists(&*cfg.local).unwrap_or(false);
        Ok(AdoptedJournal { backup_dir, ready })
    }

    /// Steps 1–9 of the switch. `Err` only before the journal moves.
    fn adopt_inner(
        &self,
        pending: Option<&Credential>,
        lock_vault: &dyn Fn() -> Result<VaultUndo, String>,
    ) -> Result<(Option<Arc<Worker>>, Option<String>), String> {
        let cfg = &self.config;
        let old = cfg.credentials.load().map_err(|e| e.to_string())?;
        let credential = match (pending, &old) {
            (Some(pending), _) => pending.clone(),
            (None, Some(saved)) => saved.clone(),
            (None, None) => return Err(NOTHING_CONNECTED.to_owned()),
        };
        let provider = self.provider(&credential.provider)?;
        let remote = provider.remote(credential.app.as_deref(), &credential.token)?;
        match probe_key(&*cfg.local, &*remote).map_err(|e| e.to_string())? {
            KeyProbe::Different => {}
            _ => return Err("This cloud holds the same journal as this device.".to_owned()),
        }

        let _turn = self.turn();
        let undo = lock_vault()?;
        let had_worker = cfg.slot.take();
        if let Some(worker) = &had_worker {
            worker.stop();
        }
        // Puts everything back as it was: the vault, and the old worker for
        // the saved login. Best effort, like the restart at the end.
        let roll_back = |undo: VaultUndo| {
            undo();
            if had_worker.is_some() {
                if let Some(saved) = &old {
                    self.restart_old_worker(saved, &_turn);
                }
            }
        };

        if let Err(e) = erase_state(&cfg.state_path) {
            roll_back(undo);
            return Err(format!("could not clear the old sync state: {e}"));
        }
        if pending.is_some() {
            if let Err(e) = cfg.credentials.save(&credential) {
                roll_back(undo);
                return Err(e.to_string());
            }
        }
        let backup_dir = match move_journal_aside(
            &cfg.journal_dir,
            &chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
        ) {
            Ok(dir) => dir,
            Err(e) => {
                if pending.is_some() {
                    let restored = match &old {
                        Some(saved) => cfg.credentials.save(saved),
                        None => cfg.credentials.clear(),
                    };
                    if let Err(e) = restored {
                        log::warn!("could not restore the saved login: {e}");
                    }
                }
                roll_back(undo);
                return Err(e);
            }
        };
        if let Some(dir) = &backup_dir {
            log::info!("moved the journal to {dir}");
        }
        // The vault stays locked: `undo` is dropped unused.
        drop(undo);

        let worker = match self.start_worker_locked(provider, remote, &_turn) {
            Ok(worker) => Some(worker),
            Err(e) => {
                log::warn!("could not start sync after switching journals: {e}");
                None
            }
        };
        if pending.is_some() {
            if let Some(old) = old {
                if old.token.expose() != credential.token.expose() {
                    self.revoke_later(old);
                }
            }
        }
        Ok((worker, backup_dir))
    }

    /// Starts a worker for the saved login again after a switch that did not
    /// happen. Only logged on failure.
    fn restart_old_worker(&self, saved: &Credential, turn: &std::sync::MutexGuard<'_, ()>) {
        let started = self.provider(&saved.provider).and_then(|provider| {
            let remote = provider.remote(saved.app.as_deref(), &saved.token)?;
            self.start_worker_locked(provider, remote, turn)
        });
        if let Err(e) = started {
            log::warn!("could not restart sync after a switch that did not happen: {e}");
        }
    }

    /// Stops syncing and forgets the account: the credential and the engine's
    /// state go. The journal on this device is not touched.
    pub fn disconnect(&self) -> Result<(), String> {
        self.discard_pending();
        let _turn = self.turn();
        // Read before it is cleared: once `clear` succeeds there is nothing
        // left to revoke.
        let saved = self.config.credentials.load().ok().flatten();
        // Out of the slot before it is stopped: stopping waits for a pass, and
        // a pass's resolver saves through the store whose callback takes the
        // slot's lock.
        if let Some(worker) = self.config.slot.take() {
            worker.stop();
        }
        let login = self.config.credentials.clear().map_err(|e| e.to_string());
        let state = erase_state(&self.config.state_path)
            .map_err(|e| format!("could not remove the sync state: {e}"));
        (self.coalescer.poker())();
        if login.is_ok() {
            if let Some(credential) = saved {
                self.revoke_later(credential);
            }
        }
        login.and(state)
    }

    fn turn(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts a worker for `credential`, replacing one that is running.
    ///
    /// The engine's state — a cursor and revisions — describes one account's
    /// remote, so it is kept only while that account stays the one: the same
    /// provider, the same account id *and* the same app (another app is
    /// another app folder). Replacing a running worker
    /// (reconnecting after a revoked login) and `resuming` in a new run of an
    /// account that was already connected keep it. Anything else throws it
    /// away: connecting from nothing, to a different provider, or as a
    /// different account of the same one. Kept, another account's state is
    /// data loss, not only a wrong status: its cursor is refused, the engine
    /// lists the new remote from scratch, and every blob it has a base for and
    /// does not find there reads as deleted remotely — and is deleted here.
    fn install(
        &self,
        provider: &dyn Provider,
        credential: &Credential,
        login: Login,
    ) -> Result<Arc<Worker>, String> {
        // Resolved before `turn()`: a provider's `remote` may read settings,
        // and `DeviceSettingsState::update` holds those while it waits for the
        // `lifecycle` lock. It also fails before anything is stopped, erased
        // or saved.
        let remote = provider.remote(credential.app.as_deref(), &credential.token)?;
        self.install_with(provider, credential, login, remote)
    }

    /// [`install`](Self::install) with the remote already resolved.
    fn install_with(
        &self,
        provider: &dyn Provider,
        credential: &Credential,
        login: Login,
        remote: Arc<dyn RemoteStore>,
    ) -> Result<Arc<Worker>, String> {
        let turn = self.turn();
        self.replace_login_locked(credential, login, &turn)?;
        self.start_worker_locked(provider, remote, &turn)
    }

    /// The first half of [`install`](Self::install): the `keep_state`
    /// decision, stopping the running worker, erasing the state and saving a
    /// new login.
    fn replace_login_locked(
        &self,
        credential: &Credential,
        login: Login,
        _turn: &std::sync::MutexGuard<'_, ()>,
    ) -> Result<(), String> {
        let cfg = &self.config;
        let keep_state = match login {
            Login::Saved => true,
            // Unreadable counts as different, and so does a saved login with
            // no account or no app recorded: losing the state is safe.
            Login::New => matches!(
                cfg.credentials.load(),
                Ok(Some(saved)) if saved.provider == credential.provider
                    && saved.account.is_some()
                    && saved.account == credential.account
                    && saved.app == credential.app
            ),
        };
        if let Some(old) = cfg.slot.take() {
            old.stop();
        }
        // Erased *before* the new login is saved. A failure, or a crash,
        // between the two must never leave a credential naming one account
        // beside state describing another: the next run would read the new
        // remote's listing as every entry deleted, and delete them here too.
        // The reverse — state gone, credential unchanged — is safe.
        if !keep_state {
            erase_state(&cfg.state_path)
                .map_err(|e| format!("could not clear the old sync state: {e}"))?;
        }
        if matches!(login, Login::New) {
            cfg.credentials
                .save(credential)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// The second half of [`install`](Self::install): opens the engine and
    /// starts a worker, which goes in the slot.
    fn start_worker_locked(
        &self,
        provider: &dyn Provider,
        remote: Arc<dyn RemoteStore>,
        _turn: &std::sync::MutexGuard<'_, ()>,
    ) -> Result<Arc<Worker>, String> {
        let cfg = &self.config;
        let poke = self.coalescer.poker();
        let on_status = {
            let poke = poke.clone();
            Box::new(move |_: &SyncStatus| poke())
        };
        // Media blobs are written once and never rewritten with different
        // content, so the engine need not re-read them every pass. Snapshots
        // are excluded: they are written with `put`, not `put_new`, so nothing
        // enforces that a name is never rewritten with different content.
        let engine = SyncEngine::open(
            cfg.local.clone(),
            remote,
            cfg.state_path.clone(),
            Box::new(is_media),
            on_status,
        )
        .map_err(|e| e.to_string())?;
        // Read under the same `turn()` lock `set_interval` stores under: see
        // its doc comment for the race this closes.
        let timing = *self.timing.lock().unwrap_or_else(PoisonError::into_inner);
        // A failure here leaves the credential already saved above and sync
        // off: the next launch's `resume` retries starting the worker.
        let worker = Worker::start(
            Arc::new(engine),
            Arc::new(VaultResolver::new(cfg.vault.clone())),
            cfg.events.clone(),
            timing,
            poke.clone(),
            (provider.id(), provider.name()),
        )
        .map_err(|e| format!("could not start sync: {e}"))?;
        cfg.slot.set(worker.clone());
        poke();
        Ok(worker)
    }
}

const NOTHING_CONNECTED: &str = "Nothing is connected.";

/// Moves the journal directory to `entries.backup-<timestamp>` beside it.
/// Nothing is deleted. `None` when there was no directory to move.
///
/// `fs::rename` silently replaces an *empty* directory on Unix, so the
/// sibling's absence is checked first rather than trusted to the rename.
fn move_journal_aside(
    journal_dir: &std::path::Path,
    stamp: &str,
) -> Result<Option<String>, String> {
    if std::fs::symlink_metadata(journal_dir).is_err() {
        return Ok(None);
    }
    let sibling = journal_dir.with_file_name(format!("entries.backup-{stamp}"));
    if std::fs::symlink_metadata(&sibling).is_ok() {
        return Err(format!("{} already exists", sibling.display()));
    }
    std::fs::rename(journal_dir, &sibling)
        .map_err(|e| format!("could not move the journal aside: {e}"))?;
    Ok(Some(sibling.display().to_string()))
}

/// The providers this build can connect to, for the connect dialog.
#[tauri::command]
pub async fn list_providers(
    control: State<'_, Arc<SyncControl>>,
) -> Result<Vec<ProviderInfo>, String> {
    Ok(control.providers())
}

/// Connects to the provider with this id (see [`Provider::id`]).
#[tauri::command]
pub async fn connect_remote(
    provider: String,
    control: State<'_, Arc<SyncControl>>,
) -> Result<ConnectOutcome, String> {
    let control = control.inner().clone();
    crate::blocking(move || control.connect(&provider)).await
}

/// Switches to the cloud's journal: this device's is moved aside and the
/// vault is locked. See [`SyncControl::adopt_remote`].
#[tauri::command]
pub async fn adopt_remote_journal(
    app: tauri::AppHandle,
    control: State<'_, Arc<SyncControl>>,
) -> Result<AdoptedJournal, String> {
    use tauri::Manager;
    let control = control.inner().clone();
    crate::blocking(move || {
        control.adopt_remote(&|| {
            let drained = crate::vault::take_drained(
                &app.state::<crate::vault::VaultState>(),
                crate::vault::DRAIN_TIMEOUT,
            )?;
            let app = app.clone();
            Ok(Box::new(move || {
                drained.undo(&app.state::<crate::vault::VaultState>())
            }))
        })
    })
    .await
}

/// Cancels a connection that found a different journal.
#[tauri::command]
pub async fn discard_pending_connection(
    control: State<'_, Arc<SyncControl>>,
) -> Result<(), String> {
    let control = control.inner().clone();
    crate::blocking(move || control.discard_pending_connection()).await
}

/// Whether the connected cloud holds a journal.
#[tauri::command]
pub async fn cloud_has_journal(control: State<'_, Arc<SyncControl>>) -> Result<bool, String> {
    let control = control.inner().clone();
    crate::blocking(move || control.cloud_has_journal()).await
}

/// Forgets the account. Local data is untouched.
#[tauri::command]
pub async fn disconnect_remote(control: State<'_, Arc<SyncControl>>) -> Result<(), String> {
    let control = control.inner().clone();
    crate::blocking(move || control.disconnect()).await
}

#[tauri::command]
pub async fn sync_now(control: State<'_, Arc<SyncControl>>) -> Result<(), String> {
    let control = control.inner().clone();
    crate::blocking(move || control.sync_now()).await
}

/// The same payload `sync://status` carries, for the frontend's first render.
#[tauri::command]
pub async fn sync_status(
    control: State<'_, Arc<SyncControl>>,
) -> Result<SyncStatusPayload, String> {
    Ok(control.payload())
}
