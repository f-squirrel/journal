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

use journal_api::{ProviderInfo, SyncStatusPayload};
use journal_credential::{Credential, CredentialStore, RefreshToken};
use journal_remote::RemoteStore;
use journal_store::{BlobName, BlobStore};
use journal_sync_engine::{erase_state, SyncEngine, SyncStatus};
use tauri::State;
use url::Url;

pub use dropbox::Dropbox;
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

    /// Starts connecting. `redirect_uri` is where the provider will send the
    /// browser back to.
    fn begin(&self, redirect_uri: &str) -> Result<Connection, String>;

    /// The provider's stable id for the account `token` acts as. Called once
    /// per connection, after the code is redeemed, and saved with the login:
    /// sync state describes one account's remote, so a different account of
    /// the same provider must start from a clean state. Blocks on the network.
    fn account(&self, token: &RefreshToken) -> Result<String, String>;

    /// The remote for an account that has been connected.
    fn remote(&self, token: &RefreshToken) -> Result<Arc<dyn RemoteStore>, String>;

    /// Revokes `token` at the provider, so a login this app no longer uses
    /// stops working there too (SEC-2).
    ///
    /// Best effort: it blocks on the network, and a failure is only logged by
    /// the caller, never surfaced to the user — the local disconnect or
    /// reconnect has already succeeded by the time this runs. The default does
    /// nothing, for a provider with no revoke endpoint.
    fn revoke(&self, token: &RefreshToken) -> Result<(), String> {
        let _ = token;
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
    /// The engine's per-device state. Outside the store, so it is never synced.
    pub state_path: PathBuf,
    /// Shows the user the provider's page.
    pub open_url: OpenUrl,
    pub timing: Timing,
}

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
                if let Err(e) = provider.revoke(&credential.token) {
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
            })
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
    pub fn connect(&self, provider_id: &str) -> Result<(), String> {
        let cfg = &self.config;
        let provider = self.provider(provider_id)?;
        let port = provider.redirect_port().unwrap_or(0);
        let (listener, redirect_uri) = oauth::bind(port, provider.name())?;
        let Connection { url, state, redeem } = provider.begin(&redirect_uri)?;
        (cfg.open_url)(&url).map_err(|e| format!("could not open the browser: {e}"))?;
        let code = oauth::await_code(&listener, &state, LOGIN_TIMEOUT, provider.name())?;
        drop(listener);
        let token = redeem(&code)?;
        let account = provider.account(&token)?;
        let credential = Credential::new(provider.id(), token)
            .try_with_account(account)
            .ok_or_else(|| format!("{} returned an unusable account id", provider.name()))?;

        // Read before `install` so a failed install (the old login may still
        // be the one in use) revokes nothing.
        let old = cfg.credentials.load().ok().flatten();
        let worker = self.install(provider, &credential, Login::New)?;
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
        Ok(())
    }

    /// Stops syncing and forgets the account: the credential and the engine's
    /// state go. The journal on this device is not touched.
    pub fn disconnect(&self) -> Result<(), String> {
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
        let result = login.and(state);
        if result.is_ok() {
            if let Some(credential) = saved {
                self.revoke_later(credential);
            }
        }
        result
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
    /// provider *and* the same account id. Replacing a running worker
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
        let cfg = &self.config;
        let _turn = self.turn();

        let keep_state = match login {
            Login::Saved => true,
            // Unreadable counts as different, and so does a saved login with
            // no account recorded: losing the state is safe.
            Login::New => matches!(
                cfg.credentials.load(),
                Ok(Some(saved)) if saved.provider == credential.provider
                    && saved.account.is_some()
                    && saved.account == credential.account
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

        let remote = provider.remote(&credential.token)?;
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
            provider.name(),
        );
        cfg.slot.set(worker.clone());
        poke();
        Ok(worker)
    }
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
) -> Result<(), String> {
    let control = control.inner().clone();
    crate::blocking(move || control.connect(&provider)).await
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
