//! Cloud sync, wired into the app: the worker thread, the connect flow, the
//! status the frontend sees, and the commands that drive them.
//!
//! The crates below do the work — `journal-sync` reconciles blobs,
//! `journal-dropbox` speaks to Dropbox — and know nothing of Tauri. This module
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

pub mod oauth;
pub mod resolver;
pub mod status;
pub mod worker;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use journal_dropbox::{authorize_url, finish, DropboxRemote, RefreshToken, TokenStore};
use journal_entry::SyncStatusPayload;
use journal_remote::RemoteStore;
use journal_store::{BlobName, BlobStore};
use journal_sync::{erase_state, SyncEngine, SyncStatus};
use tauri::State;
use url::Url;

use resolver::{VaultHandle, VaultResolver};
use status::Coalescer;
use worker::{Timing, Worker};

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
    pub redeem: Redeem,
}

/// What the app needs of a cloud provider beyond the `RemoteStore` itself: how
/// to connect an account, and how to reach it afterwards. Dropbox is the only
/// one; the trait is what lets the tests stand another in.
pub trait Provider: Send + Sync {
    /// Starts connecting. `redirect_uri` is where the provider will send the
    /// browser back to.
    fn begin(&self, redirect_uri: &str) -> Result<Connection, String>;

    /// The remote for an account that has been connected.
    fn remote(&self, token: &RefreshToken) -> Result<Arc<dyn RemoteStore>, String>;
}

/// Dropbox, under the app key this build was made with.
///
/// The key is public — PKCE means the app carries no secret — but it names an
/// app registered by whoever ships the build, so it is supplied at build time:
/// `DROPBOX_APP_KEY=… make run`. A build without one runs, with sync off and a
/// message saying why when connecting is tried.
pub struct Dropbox {
    app_key: Option<&'static str>,
}

impl Dropbox {
    pub fn from_build() -> Self {
        Self {
            app_key: option_env!("DROPBOX_APP_KEY").filter(|key| !key.is_empty()),
        }
    }

    fn key(&self) -> Result<&'static str, String> {
        self.app_key.ok_or_else(|| {
            "Dropbox is not set up in this build: register an app at dropbox.com/developers, \
             allow the redirect URI http://127.0.0.1:53682/, and build with DROPBOX_APP_KEY set"
                .to_owned()
        })
    }
}

impl Provider for Dropbox {
    fn begin(&self, redirect_uri: &str) -> Result<Connection, String> {
        let (url, pending) = authorize_url(self.key()?, redirect_uri);
        Ok(Connection {
            url,
            state: pending.state().to_owned(),
            redeem: Box::new(move |code| finish(pending, code).map_err(|e| e.to_string())),
        })
    }

    fn remote(&self, token: &RefreshToken) -> Result<Arc<dyn RemoteStore>, String> {
        Ok(Arc::new(DropboxRemote::new(self.key()?, token)))
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
            Some(worker) => {
                status::payload(true, &worker.engine().status(), worker.detail().as_deref())
            }
            None => status::payload(false, &SyncStatus::default(), None),
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
    pub provider: Arc<dyn Provider>,
    pub tokens: Arc<dyn TokenStore>,
    /// The engine's per-device state. Outside the store, so it is never synced.
    pub state_path: PathBuf,
    /// Shows the user the provider's page.
    pub open_url: OpenUrl,
    pub redirect_port: u16,
    pub timing: Timing,
}

/// The app's sync, managed by Tauri as an `Arc` so a command can hand it to the
/// blocking pool.
pub struct SyncControl {
    config: SyncConfig,
    coalescer: Coalescer,
    /// Connecting, reconnecting and disconnecting take turns. Not held while
    /// waiting for the browser, which can take minutes.
    lifecycle: Mutex<()>,
}

impl SyncControl {
    pub fn new(config: SyncConfig) -> Self {
        let coalescer = {
            let slot = config.slot.clone();
            Coalescer::new(
                STATUS_INTERVAL,
                move || slot.payload(),
                config.events.clone(),
            )
        };
        Self {
            config,
            coalescer,
            lifecycle: Mutex::new(()),
        }
    }

    /// Starts syncing if an account was connected in an earlier run. Scans the
    /// whole local store, so call it off the main thread.
    pub fn resume(&self) {
        match self.config.tokens.load() {
            Ok(Some(token)) => {
                if let Err(e) = self.install(&token, true) {
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
    /// browser to come back, saves the credential and starts syncing.
    ///
    /// Works before there is a vault: on a second device this is how the
    /// journal's key arrives. It runs one pass before returning, so that the
    /// caller can ask `has_vault` again and find it. That pass failing does not
    /// fail the connection — the credential is good — and the status says why.
    pub fn connect(&self) -> Result<(), String> {
        let cfg = &self.config;
        let (listener, redirect_uri) = oauth::bind(cfg.redirect_port)?;
        let Connection { url, state, redeem } = cfg.provider.begin(&redirect_uri)?;
        (cfg.open_url)(&url).map_err(|e| format!("could not open the browser: {e}"))?;
        let code = oauth::await_code(&listener, &state, LOGIN_TIMEOUT)?;
        drop(listener);
        let token = redeem(&code)?;
        cfg.tokens.save(&token).map_err(|e| e.to_string())?;

        let worker = self.install(&token, false)?;
        if let Err(e) = worker.sync_now() {
            log::warn!("the first sync after connecting failed: {e}");
        }
        Ok(())
    }

    /// Stops syncing and forgets the account: the credential and the engine's
    /// state go. The journal on this device is not touched.
    pub fn disconnect(&self) -> Result<(), String> {
        let _turn = self.turn();
        // Out of the slot before it is stopped: stopping waits for a pass, and
        // a pass's resolver saves through the store whose callback takes the
        // slot's lock.
        if let Some(worker) = self.config.slot.take() {
            worker.stop();
        }
        let tokens = self.config.tokens.clear().map_err(|e| e.to_string());
        let state = erase_state(&self.config.state_path)
            .map_err(|e| format!("could not remove the sync state: {e}"));
        (self.coalescer.poker())();
        tokens.and(state)
    }

    fn turn(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts a worker for `token`, replacing one that is running.
    ///
    /// The engine's state is kept when it is still the right one: replacing a
    /// running worker (reconnecting after a revoked login), or `resuming` in a
    /// new run of an account that was already connected. Connecting from
    /// nothing throws away whatever state is lying around, because it
    /// describes some earlier account's remote and the engine would compare
    /// this one against it.
    fn install(&self, token: &RefreshToken, resuming: bool) -> Result<Arc<Worker>, String> {
        let cfg = &self.config;
        let _turn = self.turn();
        match cfg.slot.take() {
            Some(old) => old.stop(),
            None if resuming => {}
            None => erase_state(&cfg.state_path)
                .map_err(|e| format!("could not clear the old sync state: {e}"))?,
        }

        let remote = cfg.provider.remote(token)?;
        let poke = self.coalescer.poker();
        let on_status = {
            let poke = poke.clone();
            Box::new(move |_: &SyncStatus| poke())
        };
        let engine = SyncEngine::open(cfg.local.clone(), remote, cfg.state_path.clone(), on_status)
            .map_err(|e| e.to_string())?;
        let worker = Worker::start(
            Arc::new(engine),
            Arc::new(VaultResolver::new(cfg.vault.clone())),
            cfg.events.clone(),
            cfg.timing,
            poke.clone(),
        );
        cfg.slot.set(worker.clone());
        poke();
        Ok(worker)
    }
}

#[tauri::command]
pub async fn connect_dropbox(control: State<'_, Arc<SyncControl>>) -> Result<(), String> {
    let control = control.inner().clone();
    crate::blocking(move || control.connect()).await
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
