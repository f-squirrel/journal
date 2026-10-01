//! The sync wiring end to end, over Tauri's mock runtime and an in-memory
//! remote standing in for Dropbox: the commands, over the real IPC path, on
//! the real vault, worker and engine.
//!
//! Each [`Rig`] is one device — its own app, journal directory and login. Two
//! rigs over one `MemoryRemote` are two devices of one account.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use journal_api::SyncState;
use journal_credential::{Credential, CredentialStoreError, FileCredentialStore};
use journal_fs_store::FsStore;
use journal_remote::{
    ChangePage, Cursor, MemoryRemote, Precondition, RemoteError, RemoteStore, Revision,
};
use journal_store::BlobName;
use journal_sync_engine::NotifyingStore;
use serde_json::{json, Value};
use tauri::test::MockRuntime;
use tauri::utils::acl::ExecutionContext;
use tauri::Manager;

use super::*;
use crate::vault::VaultState;
use crate::{entry, settings, vault};

const PASSPHRASE: &str = "correct horse";
const COMMANDS: [&str; 14] = [
    "has_vault",
    "unlock",
    "lock",
    "list_entries",
    "get_entry",
    "save_entry",
    "delete_entry",
    "get_settings",
    "update_settings",
    "list_providers",
    "connect_remote",
    "disconnect_remote",
    "sync_now",
    "sync_status",
];

/// Records what the app would have sent the webview.
#[derive(Default)]
struct Recorder {
    statuses: Mutex<Vec<SyncStatusPayload>>,
    changed: AtomicUsize,
}

impl Events for Recorder {
    fn status(&self, payload: &SyncStatusPayload) {
        self.statuses.lock().unwrap().push(payload.clone());
    }

    fn changed(&self) {
        self.changed.fetch_add(1, Ordering::SeqCst);
    }
}

/// A provider that needs no network: the "browser" answers with a fixed code,
/// and the account and its remote are whatever it was last given.
struct Fake {
    id: &'static str,
    account: Mutex<(&'static str, Arc<dyn RemoteStore>)>,
    /// Each connection gets its own token, so a test can tell which one a
    /// revoke named.
    next_token: AtomicUsize,
    /// Every token `revoke` was called with, in order.
    revoked: Mutex<Vec<String>>,
    /// Makes the next `revoke` call fail, then resets itself: a revoke is
    /// fire-and-forget, so a test checks it does not stop the local change.
    fail_next_revoke: AtomicBool,
    /// The app the next `begin` grants under, like a Dropbox app key.
    app: Mutex<Option<&'static str>>,
    /// The app each `account`/`remote`/`revoke` call was given, in order.
    apps_seen: Mutex<Vec<(&'static str, Option<String>)>>,
}

impl Fake {
    fn new(id: &'static str, remote: Arc<dyn RemoteStore>) -> Arc<Self> {
        Arc::new(Self {
            id,
            account: Mutex::new(("account-1", remote)),
            next_token: AtomicUsize::new(1),
            revoked: Mutex::new(Vec::new()),
            fail_next_revoke: AtomicBool::new(false),
            app: Mutex::new(Some("fake-app")),
            apps_seen: Mutex::new(Vec::new()),
        })
    }

    /// The next connection is granted under `app`.
    fn use_app(&self, app: &'static str) {
        *self.app.lock().unwrap() = Some(app);
    }

    /// The app each call of `method` was given, in order.
    fn apps_seen(&self, method: &str) -> Vec<Option<String>> {
        self.apps_seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| *m == method)
            .map(|(_, app)| app.clone())
            .collect()
    }

    fn see(&self, method: &'static str, app: Option<&str>) {
        self.apps_seen
            .lock()
            .unwrap()
            .push((method, app.map(str::to_owned)));
    }

    fn provider(id: &'static str, remote: Arc<dyn RemoteStore>) -> Arc<dyn Provider> {
        Self::new(id, remote)
    }

    /// The user signs in to the provider as someone else next time.
    fn sign_in_as(&self, account: &'static str, remote: Arc<dyn RemoteStore>) {
        *self.account.lock().unwrap() = (account, remote);
    }

    /// The tokens `revoke` has been called with so far.
    fn revoked(&self) -> Vec<String> {
        self.revoked.lock().unwrap().clone()
    }

    fn fail_next_revoke(&self) {
        self.fail_next_revoke.store(true, Ordering::SeqCst);
    }
}

impl Provider for Fake {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        "Fake Cloud"
    }

    fn begin(&self, redirect_uri: &str) -> Result<Connection, String> {
        let mut url = Url::parse("https://provider.example/authorize").unwrap();
        url.query_pairs_mut()
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", "fake-state");
        // A fresh token per connection, like a real OAuth grant: reconnecting
        // the same account still gets a new one, which is exactly the login
        // SEC-2 revokes.
        let token = format!("refresh-{}", self.next_token.fetch_add(1, Ordering::SeqCst));
        Ok(Connection {
            url,
            state: "fake-state".to_owned(),
            app: self.app.lock().unwrap().map(str::to_owned),
            redeem: Box::new(move |code| match code {
                "the-code" => Ok(RefreshToken::new(token)),
                other => Err(format!("unexpected code {other}")),
            }),
        })
    }

    fn account(&self, app: Option<&str>, _: &RefreshToken) -> Result<String, String> {
        self.see("account", app);
        Ok(self.account.lock().unwrap().0.to_owned())
    }

    fn remote(&self, app: Option<&str>, _: &RefreshToken) -> Result<Arc<dyn RemoteStore>, String> {
        self.see("remote", app);
        Ok(self.account.lock().unwrap().1.clone())
    }

    fn revoke(&self, app: Option<&str>, token: &RefreshToken) -> Result<(), String> {
        self.see("revoke", app);
        self.revoked.lock().unwrap().push(token.expose().to_owned());
        if self.fail_next_revoke.swap(false, Ordering::SeqCst) {
            return Err("could not reach Fake Cloud".to_owned());
        }
        Ok(())
    }
}

#[derive(Default)]
struct RefusingClearCredentials(Mutex<Option<Credential>>);

impl CredentialStore for RefusingClearCredentials {
    fn load(&self) -> Result<Option<Credential>, CredentialStoreError> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn save(&self, credential: &Credential) -> Result<(), CredentialStoreError> {
        *self.0.lock().unwrap() = Some(credential.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::Other(Box::new(io::Error::other(
            "credential clear refused",
        ))))
    }
}

/// A remote that answers a cursor it did not issue the way Dropbox does, with
/// `reset` (`CursorExpired`), where `MemoryRemote` reports an error. It counts
/// the listings made from scratch, which is how a test sees that the engine
/// started over: a kept state starts from its cursor.
struct Forgetful(Arc<MemoryRemote>, AtomicUsize);

impl Forgetful {
    fn new(inner: Arc<MemoryRemote>) -> Arc<Self> {
        Arc::new(Self(inner, AtomicUsize::new(0)))
    }

    fn full_listings(&self) -> usize {
        self.1.load(Ordering::SeqCst)
    }
}

impl RemoteStore for Forgetful {
    fn changes(&self, since: Option<&Cursor>) -> Result<ChangePage, RemoteError> {
        if since.is_none() {
            self.1.fetch_add(1, Ordering::SeqCst);
        }
        match self.0.changes(since) {
            Err(RemoteError::Other(_)) if since.is_some() => Err(RemoteError::CursorExpired),
            other => other,
        }
    }

    fn get(&self, name: &BlobName) -> Result<Option<(Vec<u8>, Revision)>, RemoteError> {
        self.0.get(name)
    }

    fn put(
        &self,
        name: &BlobName,
        bytes: &[u8],
        pre: Precondition,
    ) -> Result<Revision, RemoteError> {
        self.0.put(name, bytes, pre)
    }

    fn delete(&self, name: &BlobName, pre: Precondition) -> Result<(), RemoteError> {
        self.0.delete(name, pre)
    }

    fn content_hash(&self, bytes: &[u8]) -> Option<String> {
        self.0.content_hash(bytes)
    }
}

/// The user's browser: goes to the provider's page and is sent straight back
/// to the redirect with a code.
fn browser(url: &Url) -> Result<(), String> {
    let param = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
            .unwrap()
    };
    let redirect = Url::parse(&param("redirect_uri")).unwrap();
    let port = redirect.port().unwrap();
    let state = param("state");
    thread::spawn(move || {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(
            stream,
            "GET /?code=the-code&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
    });
    Ok(())
}

/// A remote whose reads can be held shut, to freeze a pass mid-I/O.
struct Gated {
    inner: Arc<MemoryRemote>,
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gated {
    fn new(inner: Arc<MemoryRemote>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            open: Mutex::new(true),
            changed: Condvar::new(),
        })
    }

    fn set_open(&self, open: bool) {
        *self.open.lock().unwrap() = open;
        self.changed.notify_all();
    }

    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
}

impl RemoteStore for Gated {
    fn changes(&self, since: Option<&Cursor>) -> Result<ChangePage, RemoteError> {
        self.wait();
        self.inner.changes(since)
    }

    fn get(&self, name: &BlobName) -> Result<Option<(Vec<u8>, Revision)>, RemoteError> {
        self.inner.get(name)
    }

    fn put(
        &self,
        name: &BlobName,
        bytes: &[u8],
        pre: Precondition,
    ) -> Result<Revision, RemoteError> {
        self.inner.put(name, bytes, pre)
    }

    fn delete(&self, name: &BlobName, pre: Precondition) -> Result<(), RemoteError> {
        self.inner.delete(name, pre)
    }

    fn content_hash(&self, bytes: &[u8]) -> Option<String> {
        self.inner.content_hash(bytes)
    }
}

/// Slow enough that a test can look at a save before it uploads, and at a
/// failure before it is retried: the tests that want a pass ask for one.
fn timing() -> Timing {
    Timing {
        debounce: Duration::from_secs(5),
        max_debounce: Duration::from_secs(10),
        interval: Some(Duration::from_secs(3600)),
        backoff_base: Duration::from_secs(5),
        backoff_max: Duration::from_secs(5),
    }
}

/// One device: an app over the real commands, with its own journal directory.
struct Rig {
    // Kept alive: dropping the app drops its managed state.
    app: tauri::App<MockRuntime>,
    webview: tauri::WebviewWindow<MockRuntime>,
    control: Arc<SyncControl>,
    events: Arc<Recorder>,
    dir: PathBuf,
}

impl Rig {
    fn new(remote: Arc<dyn RemoteStore>) -> Self {
        Self::with_providers(vec![Fake::provider("fake", remote)])
    }

    fn with_providers(providers: Vec<Arc<dyn Provider>>) -> Self {
        Self::build(providers, timing())
    }

    fn build(providers: Vec<Arc<dyn Provider>>, timing: Timing) -> Self {
        Self::build_with_credentials(providers, timing, None)
    }

    fn build_with_credentials(
        providers: Vec<Arc<dyn Provider>>,
        timing: Timing,
        credentials: Option<Arc<dyn CredentialStore>>,
    ) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("journal-sync-app-{}-{n}", std::process::id()));

        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        for command in COMMANDS {
            // A mock context carries no ACL; see the test in `lib.rs`.
            context
                .runtime_authority_mut()
                .__allow_command(command.into(), ExecutionContext::Local);
        }
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![
                vault::has_vault,
                vault::unlock,
                vault::lock,
                entry::list_entries,
                entry::get_entry,
                entry::save_entry,
                entry::delete_entry,
                settings::get_settings,
                settings::update_settings,
                list_providers,
                connect_remote,
                disconnect_remote,
                sync_now,
                sync_status
            ])
            .build(context)
            .expect("mock app builds");

        // The same wiring as `run`.
        let entries = FsStore::new(dir.join("entries"));
        let slot = Arc::new(Slot::default());
        let notifying = {
            let slot = slot.clone();
            NotifyingStore::new(entries.clone(), move |name| slot.dirty(name))
        };
        app.manage(VaultState {
            store: Arc::new(notifying),
            vault: Mutex::new(None),
            generation: std::sync::atomic::AtomicU64::new(0),
        });
        app.manage(Arc::new(crate::settings::DeviceSettingsState::load(
            dir.join("device-settings.json"),
        )));
        let handle = app.handle().clone();
        let events = Arc::new(Recorder::default());
        let credentials = credentials.unwrap_or_else(|| {
            Arc::new(FileCredentialStore::new(dir.join("config").join("login")))
        });
        let control = Arc::new(
            SyncControl::new(SyncConfig {
                slot,
                local: Arc::new(entries),
                journal_dir: dir.join("entries"),
                vault: Arc::new(move || crate::vault::get(&handle.state::<VaultState>()).ok()),
                events: events.clone(),
                providers,
                credentials,
                state_path: dir.join("sync-state.json"),
                open_url: Arc::new(browser),
                timing,
            })
            .unwrap(),
        );
        app.manage(control.clone());

        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");
        Self {
            app,
            webview,
            control,
            events,
            dir,
        }
    }

    /// A command over the real IPC path.
    fn invoke(&self, cmd: &str, body: Value) -> Result<Value, Value> {
        let response = tauri::test::get_ipc_response(
            &self.webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: if cfg!(any(windows, target_os = "android")) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )?;
        Ok(response.deserialize().expect("a command answers in JSON"))
    }

    fn ok(&self, cmd: &str, body: Value) -> Value {
        self.invoke(cmd, body)
            .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
    }

    fn unlock(&self) {
        self.ok("unlock", json!({ "passphrase": PASSPHRASE }));
    }

    /// The `updated_at` this device's vault currently holds for `id`, or
    /// `None` if it has no such entry yet — what an honest frontend sends as
    /// its precondition on the next save (`useAutosave`'s `base` ref).
    fn current_updated_at(&self, id: &str) -> Option<String> {
        let entry = self.ok("get_entry", json!({ "id": id }));
        (!entry.is_null()).then(|| entry["updated_at"].as_str().unwrap().to_owned())
    }

    fn save(&self, id: &str, title: &str, markdown: &str, updated_at: &str) {
        let expected = self.current_updated_at(id);
        self.ok(
            "save_entry",
            json!({
                "entry": {
                    "id": id, "title": title, "markdown": markdown, "updated_at": updated_at,
                },
                "expected_updated_at": expected,
            }),
        );
    }

    fn status(&self) -> Value {
        self.ok("sync_status", json!({}))
    }

    fn get_settings(&self) -> Value {
        self.ok("get_settings", json!({}))
    }

    fn update_settings(&self, patch: Value) -> Result<Value, Value> {
        self.invoke("update_settings", json!({ "patch": patch }))
    }

    fn titles(&self) -> Vec<String> {
        let mut titles: Vec<_> = self
            .ok("list_entries", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["title"].as_str().unwrap().to_owned())
            .collect();
        titles.sort();
        titles
    }

    fn markdown(&self, id: &str) -> String {
        self.ok("get_entry", json!({ "id": id }))["markdown"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Connects to the provider `fake`, over the loopback and the fake
    /// browser.
    fn connect(&self) {
        self.connect_to("fake");
    }

    fn connect_to(&self, provider: &str) {
        self.ok("connect_remote", json!({ "provider": provider }));
    }

    /// What the login file says: the provider it is for, if one is saved.
    fn saved_provider(&self) -> Option<String> {
        FileCredentialStore::new(self.dir.join("config").join("login"))
            .load()
            .unwrap()
            .map(|login| login.provider)
    }

    /// The token in the login file, if one is saved.
    fn saved_token(&self) -> Option<String> {
        FileCredentialStore::new(self.dir.join("config").join("login"))
            .load()
            .unwrap()
            .map(|login| login.token.expose().to_owned())
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = &self.app;
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

const ID: &str = "5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01";

fn remote_has(remote: &MemoryRemote, name: &str) -> bool {
    remote
        .get(&BlobName::try_from(name).unwrap())
        .unwrap()
        .is_some()
}

#[test]
fn sync_is_off_until_an_account_is_connected() {
    let rig = Rig::new(Arc::new(MemoryRemote::new()));

    let status = rig.status();

    assert_eq!(status["state"], "off");
    assert_eq!(status["pending_ids"], json!([]));
    assert_eq!(
        rig.invoke("sync_now", json!({})),
        Err(json!("sync is not connected"))
    );
}

#[test]
fn a_build_without_an_app_key_says_how_to_get_one() {
    let rig = Rig::with_providers(vec![Arc::new(Dropbox::new(Arc::new(|| None), None))]);

    let error = rig
        .invoke("connect_remote", json!({ "provider": "dropbox" }))
        .unwrap_err();

    assert!(
        error.as_str().unwrap().contains("DROPBOX_APP_KEY"),
        "{error}"
    );
    assert_eq!(rig.status()["state"], "off");
}

/// The user's key wins over the build's, the build's is the fallback, and
/// with neither there is nothing to connect.
#[test]
fn the_users_app_key_wins_over_the_builds() {
    let chosen =
        |key: Option<&'static str>| -> KeySource { Arc::new(move || key.map(str::to_owned)) };

    let both = Dropbox::new(chosen(Some("mine")), Some("built"));
    assert_eq!(both.app_key().as_deref(), Some("mine"));
    let built = Dropbox::new(chosen(None), Some("built"));
    assert_eq!(built.app_key().as_deref(), Some("built"));
    assert!(built.configured());

    let neither = Dropbox::new(chosen(None), None);
    assert_eq!(neither.app_key(), None);
    assert_eq!(neither.redirect_port(), None);
    assert!(!neither.configured());
    assert!(neither.key().unwrap_err().contains("Settings"));
}

/// A saved Dropbox login is used only under the key stored with it. The key
/// source panics, so this also shows it is never read.
#[test]
fn a_saved_dropbox_login_never_reads_the_live_key() {
    let dropbox = Dropbox::new(Arc::new(|| panic!("the live key was read")), None);
    let token = RefreshToken::new("refresh");

    for error in [
        dropbox.remote(None, &token).err().unwrap(),
        dropbox.revoke(None, &token).unwrap_err(),
        dropbox.account(None, &token).unwrap_err(),
    ] {
        assert!(error.contains("connect again"), "{error}");
    }
    assert!(dropbox.remote(Some("k"), &token).is_ok());
}

/// A login saved before the app was stored has no app line: it does not resume,
/// and nothing is erased or replaced on the way.
#[test]
fn a_pre_change_dropbox_login_does_not_resume() {
    let rig = Rig::with_providers(vec![Arc::new(Dropbox::new(
        Arc::new(|| Some("live".to_owned())),
        None,
    ))]);
    let login = rig.dir.join("config").join("login");
    std::fs::create_dir_all(login.parent().unwrap()).unwrap();
    std::fs::write(&login, "dropbox\nrefresh\ndbid:A\n").unwrap();
    let state = rig.dir.join("sync-state.json");
    std::fs::write(&state, "{}").unwrap();

    rig.control.resume();

    assert_eq!(rig.status()["state"], "off");
    assert!(login.exists(), "the login stays until the user reconnects");
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "{}");
}

#[test]
fn connecting_saves_the_app_the_login_was_granted_to() {
    let fake = Fake::new("fake", Arc::new(MemoryRemote::new()));
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();

    rig.connect();

    let saved = FileCredentialStore::new(rig.dir.join("config").join("login"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(saved.app.as_deref(), Some("fake-app"));
    assert_eq!(fake.apps_seen("account"), [Some("fake-app".to_owned())]);
    assert_eq!(fake.apps_seen("remote"), [Some("fake-app".to_owned())]);
}

/// Another app is another app folder: the old cursor and bases must not be
/// applied there, even for the same account.
#[test]
fn reconnecting_under_a_different_app_starts_from_clean_state() {
    let remote = Forgetful::new(Arc::new(MemoryRemote::new()));
    let fake = Fake::new("fake", remote.clone());
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    rig.connect();
    let before = remote.full_listings();

    fake.use_app("other-app");
    rig.connect();

    assert!(remote.full_listings() > before, "the state was erased");
    assert_eq!(rig.titles(), ["Today"]);
}

#[test]
fn revoke_uses_the_old_logins_app() {
    let fake = Fake::new("fake", Arc::new(MemoryRemote::new()));
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.connect();

    fake.use_app("other-app");
    rig.connect();

    wait_for("the revoke", || !fake.revoked().is_empty());
    assert_eq!(fake.apps_seen("revoke"), [Some("fake-app".to_owned())]);
}

#[test]
fn a_planned_provider_cannot_be_connected() {
    let rig = Rig::with_providers(vec![Fake::provider("one", Arc::new(MemoryRemote::new()))]);

    let error = rig
        .invoke("connect_remote", json!({ "provider": "icloud" }))
        .unwrap_err();

    assert!(error.as_str().unwrap().contains("unknown sync provider"));
    assert_eq!(rig.status()["state"], "off");
}

/// Dropbox matches the redirect against the URI registered with the app, so it
/// asks for that port; a provider that says nothing gets any free one.
#[test]
fn only_a_provider_that_needs_a_fixed_redirect_port_asks_for_one() {
    assert_eq!(
        Dropbox::new(Arc::new(|| None), Some("key")).redirect_port(),
        Some(53682)
    );
    assert_eq!(
        Fake::provider("fake", Arc::new(MemoryRemote::new())).redirect_port(),
        None
    );
}

#[test]
fn the_build_lists_its_providers_for_the_connect_dialog() {
    let rig = Rig::with_providers(vec![
        Fake::provider("one", Arc::new(MemoryRemote::new())),
        Fake::provider("two", Arc::new(MemoryRemote::new())),
    ]);

    let providers = rig.ok("list_providers", json!({}));

    assert_eq!(
        providers,
        json!([
            { "id": "one", "name": "Fake Cloud", "available": true, "configured": true },
            { "id": "two", "name": "Fake Cloud", "available": true, "configured": true },
            { "id": "google_drive", "name": "Google Drive", "available": false, "configured": false },
            { "id": "icloud", "name": "iCloud", "available": false, "configured": false },
            { "id": "webdav", "name": "WebDAV", "available": false, "configured": false },
        ])
    );
}

#[test]
fn the_status_names_the_provider_only_while_one_is_connected() {
    let rig = Rig::new(Arc::new(MemoryRemote::new()));
    assert_eq!(rig.status()["provider"], Value::Null);
    assert_eq!(rig.status()["provider_id"], Value::Null);

    rig.connect();
    assert_eq!(rig.status()["provider"], "Fake Cloud");
    assert_eq!(rig.status()["provider_id"], "fake");

    rig.ok("disconnect_remote", json!({}));
    assert_eq!(rig.status()["provider"], Value::Null);
}

#[test]
fn an_unknown_provider_is_refused_before_anything_happens() {
    let rig = Rig::new(Arc::new(MemoryRemote::new()));

    let error = rig
        .invoke("connect_remote", json!({ "provider": "nope" }))
        .unwrap_err();

    assert!(
        error.as_str().unwrap().contains("unknown sync provider"),
        "{error}"
    );
    assert_eq!(rig.status()["state"], "off");
    assert_eq!(rig.saved_provider(), None);
}

#[test]
fn the_login_remembers_which_provider_it_is_for() {
    let rig = Rig::new(Arc::new(MemoryRemote::new()));
    rig.unlock();

    rig.connect();

    assert_eq!(rig.saved_provider().as_deref(), Some("fake"));
}

#[test]
fn a_login_for_a_provider_this_build_lacks_leaves_sync_off() {
    let rig = Rig::new(Arc::new(MemoryRemote::new()));
    FileCredentialStore::new(rig.dir.join("config").join("login"))
        .save(&Credential::new("elsewhere", RefreshToken::new("refresh")))
        .unwrap();

    rig.control.resume();

    assert_eq!(rig.status()["state"], "off");
    // Not forgotten: a build that has the provider will find it.
    assert_eq!(rig.saved_provider().as_deref(), Some("elsewhere"));
}

/// The engine's state is a cursor and revisions from one provider's remote.
/// Handed to another provider's, the cursor means nothing, and the revisions
/// name blobs that are not there.
#[test]
fn connecting_another_provider_starts_from_a_clean_state() {
    let first = Arc::new(MemoryRemote::new());
    let second = Arc::new(MemoryRemote::new());
    let rig = Rig::with_providers(vec![
        Fake::provider("one", first.clone()),
        Fake::provider("two", second.clone()),
    ]);
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    rig.connect_to("one");
    assert!(remote_has(&first, &format!("{ID}.json")));

    rig.connect_to("two");

    assert_eq!(rig.saved_provider().as_deref(), Some("two"));
    assert!(
        remote_has(&second, &format!("{ID}.json")),
        "uploaded afresh"
    );
    assert_eq!(rig.status()["state"], "synced");
    assert_eq!(rig.status()["message"], Value::Null);
    assert_eq!(rig.titles(), ["Today"]);
}

/// The regression for connecting a second account of the same provider: kept,
/// the first account's state made the engine read the second's empty folder
/// as "everything was deleted", and delete the journal on this device.
#[test]
fn connecting_another_account_starts_clean_and_deletes_nothing() {
    let first = Forgetful::new(Arc::new(MemoryRemote::new()));
    let second = Arc::new(MemoryRemote::new());
    let fake = Fake::new("fake", first.clone());
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    rig.connect();
    assert!(remote_has(&first.0, &format!("{ID}.json")));

    fake.sign_in_as("account-2", Forgetful::new(second.clone()));
    rig.connect();

    assert_eq!(rig.titles(), ["Today"], "nothing deleted on this device");
    assert!(
        remote_has(&second, &format!("{ID}.json")),
        "uploaded afresh"
    );
    assert_eq!(rig.status()["state"], "synced");
    let saved = FileCredentialStore::new(rig.dir.join("config").join("login"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(saved.account.as_deref(), Some("account-2"));
}

/// The order matters more than the erase: a credential naming account B must
/// never outlive a failed erase of account A's state, or the next launch reads
/// B's remote against A's cursor and deletes the journal.
#[test]
fn a_failed_erase_leaves_the_old_login_in_place() {
    let first = Forgetful::new(Arc::new(MemoryRemote::new()));
    let fake = Fake::new("fake", first.clone());
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    rig.connect();
    // A directory where the state file was: removing it as a file fails.
    let state = rig.dir.join("sync-state.json");
    std::fs::remove_file(&state).unwrap();
    std::fs::create_dir(&state).unwrap();

    fake.sign_in_as("account-2", Forgetful::new(Arc::new(MemoryRemote::new())));
    let refused = rig
        .invoke("connect_remote", json!({ "provider": "fake" }))
        .expect_err("the state could not be cleared");

    assert!(
        refused.as_str().unwrap().contains("old sync state"),
        "{refused}"
    );
    let saved = FileCredentialStore::new(rig.dir.join("config").join("login"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(saved.account.as_deref(), Some("account-1"));
    assert_eq!(rig.titles(), ["Today"]);
}

/// The account id comes off the network; one that cannot be written on a line
/// of the login file is refused, where it used to abort the app.
#[test]
fn an_unusable_account_id_is_refused_rather_than_panicking() {
    for account in ["", "  ", "drop\rbox"] {
        let remote = Arc::new(MemoryRemote::new());
        let fake = Fake::new("fake", remote);
        fake.sign_in_as(account, Arc::new(MemoryRemote::new()));
        let rig = Rig::with_providers(vec![fake]);

        let refused = rig
            .invoke("connect_remote", json!({ "provider": "fake" }))
            .expect_err("no usable account");

        assert_eq!(
            refused, "Fake Cloud returned an unusable account id",
            "{account:?}"
        );
        assert_eq!(rig.saved_provider(), None, "{account:?}");
    }
}

/// A login saved before accounts were recorded cannot be told to be the same
/// account, so it is taken to be another one.
#[test]
fn a_saved_login_without_an_account_counts_as_another_account() {
    let remote = Forgetful::new(Arc::new(MemoryRemote::new()));
    let rig = Rig::new(remote.clone());
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    rig.connect();
    let before = remote.full_listings();
    FileCredentialStore::new(rig.dir.join("config").join("login"))
        .save(&Credential::new("fake", RefreshToken::new("refresh")))
        .unwrap();

    rig.connect();

    assert!(remote.full_listings() > before, "the state was erased");
    assert_eq!(rig.titles(), ["Today"]);
    assert_eq!(rig.status()["state"], "synced");
}

#[test]
fn reconnecting_the_same_account_keeps_the_state() {
    let remote = Forgetful::new(Arc::new(MemoryRemote::new()));
    let rig = Rig::new(remote.clone());
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    rig.connect();
    let before = remote.full_listings();

    // What a revoked login and a fresh grant look like: same provider and
    // account, worker running. The pass after it goes on from the cursor.
    rig.connect();

    assert_eq!(remote.full_listings(), before, "the state was kept");
    assert_eq!(rig.status()["state"], "synced");
}

#[test]
fn a_saved_entry_is_pending_until_it_reaches_the_remote() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    rig.connect();
    assert_eq!(rig.status()["state"], "synced");

    rig.save(ID, "Today", "first words", "2026-03-01T10:00:00Z");

    // Marked at once, before any pass: the mark is for the moment after typing.
    let status = rig.status();
    assert_eq!(status["state"], "pending");
    assert_eq!(status["pending_ids"], json!([ID]));
    assert!(!remote_has(&remote, &format!("{ID}.json")));

    // The webview is told too: throttled, but not skipped.
    wait_for("the pending event", || {
        let events = rig.events.statuses.lock().unwrap();
        events
            .last()
            .is_some_and(|s| s.state == SyncState::Pending && s.pending_ids.len() == 1)
    });

    rig.ok("sync_now", json!({}));

    let status = rig.status();
    assert_eq!(status["state"], "synced");
    assert_eq!(status["pending_ids"], json!([]));
    assert_eq!(status["message"], Value::Null);
    assert!(status["last_synced_at"].is_string());
    assert!(remote_has(&remote, &format!("{ID}.json")));
    assert!(remote_has(&remote, ".vault-key"), "the key syncs too");

    // And of the settled state, which is what the row's mark clears on.
    wait_for("the synced event", || {
        let events = rig.events.statuses.lock().unwrap();
        events
            .last()
            .is_some_and(|s| s.state == SyncState::Synced && s.pending_ids.is_empty())
    });
}

#[test]
fn a_second_device_joins_with_only_the_passphrase() {
    let remote = Arc::new(MemoryRemote::new());
    let first = Rig::new(remote.clone());
    first.unlock();
    first.save(ID, "From the laptop", "hello", "2026-03-01T10:00:00Z");
    first.connect();

    let second = Rig::new(remote.clone());
    assert_eq!(second.ok("has_vault", json!({})), json!(false));
    second.connect();

    // Connecting ran a pass, so the key is here before the user is asked.
    assert_eq!(second.ok("has_vault", json!({})), json!(true));
    second.unlock();
    assert_eq!(second.titles(), ["From the laptop"]);
    assert_eq!(second.markdown(ID), "hello");
    assert_eq!(
        second.events.changed.load(Ordering::SeqCst),
        1,
        "the pull told the frontend its list was stale"
    );
    // And nothing pulled is mistaken for a local edit.
    assert_eq!(second.status()["pending_ids"], json!([]));
}

#[test]
fn a_failed_pass_is_offline_and_the_next_one_recovers() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    rig.connect();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");

    remote.fail_next(RemoteError::Transient("no route to host".to_owned()));
    let error = rig.invoke("sync_now", json!({})).unwrap_err();

    assert!(
        error.as_str().unwrap().contains("no route to host"),
        "{error}"
    );
    let status = rig.status();
    assert_eq!(status["state"], "offline");
    assert!(status["message"]
        .as_str()
        .unwrap()
        .contains("no route to host"));
    assert_eq!(
        status["pending_ids"],
        json!([ID]),
        "what did not upload stays marked"
    );

    rig.ok("sync_now", json!({}));

    let status = rig.status();
    assert_eq!(status["state"], "synced");
    assert_eq!(status["message"], Value::Null);
    assert!(remote_has(&remote, &format!("{ID}.json")));
}

#[test]
fn revoked_credentials_stop_sync_until_the_user_asks_again() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    rig.connect();

    remote.fail_next(RemoteError::Unauthorized);
    assert!(rig.invoke("sync_now", json!({})).is_err());

    assert_eq!(rig.status()["state"], "reauth");
    // A save no longer wakes a halted worker: it would fail the same way.
    // (The pending mark still shows.)
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    assert_eq!(rig.status()["state"], "reauth");
    assert_eq!(rig.status()["pending_ids"], json!([ID]));
}

#[test]
fn disconnecting_forgets_the_account_and_leaves_the_journal() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    rig.connect();
    let login = rig.dir.join("config").join("login");
    assert!(login.exists());
    assert!(rig.dir.join("sync-state.json").exists());

    rig.ok("disconnect_remote", json!({}));

    assert_eq!(rig.status()["state"], "off");
    assert!(!login.exists(), "the login is gone");
    assert!(!rig.dir.join("sync-state.json").exists(), "and the state");
    // The journal on this device is untouched, and so is the remote's copy.
    assert_eq!(rig.titles(), ["Today"]);
    assert_eq!(rig.markdown(ID), "words");
    assert!(remote_has(&remote, &format!("{ID}.json")));
    // Nothing is connected any more.
    assert!(rig.invoke("sync_now", json!({})).is_err());
    // Disconnecting when nothing is connected is not an error.
    rig.ok("disconnect_remote", json!({}));
}

/// SEC-2: disconnecting revokes the token it forgets, so a login this app no
/// longer uses stops working at the provider too. Best effort and on a
/// background thread, so the test waits for it rather than expecting it done
/// by the time the command returns.
#[test]
fn disconnecting_revokes_the_forgotten_token() {
    let remote = Arc::new(MemoryRemote::new());
    let fake = Fake::new("fake", remote);
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.connect();
    let token = rig.saved_token().expect("a login was saved");

    rig.ok("disconnect_remote", json!({}));

    wait_for("the revoke", || !fake.revoked().is_empty());
    assert_eq!(fake.revoked(), [token]);
}

#[test]
fn disconnect_revokes_after_login_clear_even_when_state_removal_fails() {
    let fake = Fake::new("fake", Arc::new(MemoryRemote::new()));
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.connect();
    let token = rig.saved_token().expect("a login was saved");
    let login = rig.dir.join("config").join("login");
    let state = rig.dir.join("sync-state.json");
    std::fs::remove_file(&state).unwrap();
    std::fs::create_dir(&state).unwrap();

    let error = rig
        .invoke("disconnect_remote", json!({}))
        .expect_err("state removal should fail");

    assert!(error.as_str().unwrap().contains("sync state"), "{error}");
    assert!(!login.exists(), "credential clear still succeeded");
    wait_for("the revoke", || !fake.revoked().is_empty());
    assert_eq!(fake.revoked(), [token]);
}

#[test]
fn disconnect_does_not_revoke_when_credential_clear_fails() {
    let fake = Fake::new("fake", Arc::new(MemoryRemote::new()));
    let credentials = Arc::new(RefusingClearCredentials::default());
    let rig = Rig::build_with_credentials(vec![fake.clone()], timing(), Some(credentials.clone()));
    rig.unlock();
    rig.connect();

    let error = rig
        .invoke("disconnect_remote", json!({}))
        .expect_err("credential clear should fail");

    assert!(
        error.as_str().unwrap().contains("credential clear refused"),
        "{error}"
    );
    assert!(credentials.load().unwrap().is_some());
    thread::sleep(Duration::from_millis(100));
    assert!(fake.revoked().is_empty());
}

/// SEC-2: a fresh grant leaves the old token valid at the provider even for
/// the same account, so reconnecting must revoke it — but never the token
/// that was just issued.
#[test]
fn reconnecting_the_same_account_revokes_only_the_old_token() {
    let remote = Forgetful::new(Arc::new(MemoryRemote::new()));
    let fake = Fake::new("fake", remote.clone());
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.connect();
    let old_token = rig.saved_token().expect("a login was saved");

    rig.connect();
    let new_token = rig.saved_token().expect("a login was saved");

    assert_ne!(old_token, new_token, "each connection gets its own token");
    wait_for("the revoke", || !fake.revoked().is_empty());
    assert_eq!(fake.revoked(), [old_token]);
}

/// SEC-2 also covers switching to a different account of the same provider:
/// the old account's token is revoked, with the old credential's provider.
#[test]
fn switching_accounts_revokes_the_old_token() {
    let first = Arc::new(MemoryRemote::new());
    let second = Arc::new(MemoryRemote::new());
    let fake = Fake::new("fake", first);
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.connect();
    let old_token = rig.saved_token().expect("a login was saved");

    fake.sign_in_as("account-2", second);
    rig.connect();
    let new_token = rig.saved_token().expect("a login was saved");

    assert_ne!(old_token, new_token);
    wait_for("the revoke", || !fake.revoked().is_empty());
    assert_eq!(fake.revoked(), [old_token]);
}

/// Revoking is best effort: a network failure while forgetting the account
/// must not bring the credential back or fail the command.
#[test]
fn a_failed_revoke_does_not_stop_the_disconnect() {
    let remote = Arc::new(MemoryRemote::new());
    let fake = Fake::new("fake", remote);
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.connect();
    fake.fail_next_revoke();
    let login = rig.dir.join("config").join("login");

    rig.ok("disconnect_remote", json!({}));

    assert!(
        !login.exists(),
        "the login is gone even though the revoke failed"
    );
    wait_for("the failed revoke attempt", || !fake.revoked().is_empty());
}

/// `resume` reuses the login that was already in effect: nothing was replaced
/// or forgotten, so nothing is revoked.
#[test]
fn resuming_a_saved_login_revokes_nothing() {
    let remote = Arc::new(MemoryRemote::new());
    let fake = Fake::new("fake", remote.clone());
    let rig = Rig::with_providers(vec![fake.clone()]);
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    FileCredentialStore::new(rig.dir.join("config").join("login"))
        .save(&Credential::new("fake", RefreshToken::new("refresh")))
        .unwrap();

    rig.control.resume();

    wait_for("the startup pass", || rig.status()["state"] == "synced");
    // No event marks "nothing happened"; give a would-be revoke thread time
    // to run before checking that none did.
    thread::sleep(Duration::from_millis(100));
    assert!(fake.revoked().is_empty());
}

#[test]
fn an_account_connected_in_an_earlier_run_resumes() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    FileCredentialStore::new(rig.dir.join("config").join("login"))
        .save(&Credential::new("fake", RefreshToken::new("refresh")))
        .unwrap();
    assert_eq!(rig.status()["state"], "off");

    rig.control.resume();

    // The startup pass uploads what was there.
    wait_for("the startup pass", || {
        remote_has(&remote, &format!("{ID}.json")) && rig.status()["state"] == "synced"
    });
}

/// The whole of D1: two devices edit one entry; the vault on one is locked
/// when the conflict is found, so it waits for the key, and unlocking settles
/// it — the newer text under the original id, the older as a copy.
#[test]
fn a_conflict_waits_for_the_key_and_is_settled_by_unlocking() {
    let remote = Arc::new(MemoryRemote::new());
    let laptop = Rig::new(remote.clone());
    laptop.unlock();
    laptop.save(ID, "Trip", "start", "2026-03-01T10:00:00Z");
    laptop.connect();
    let phone = Rig::new(remote.clone());
    phone.connect();
    phone.unlock();
    assert_eq!(phone.markdown(ID), "start");

    // Both edit; the laptop's is the later one and reaches the remote first.
    laptop.save(ID, "Trip", "laptop's later words", "2026-03-03T10:00:00Z");
    laptop.ok("sync_now", json!({}));
    phone.save(ID, "Trip", "phone's earlier words", "2026-03-02T10:00:00Z");
    phone.ok("lock", json!({}));
    phone.ok("sync_now", json!({}));

    // Found, but not settled: settling needs plaintext.
    let status = phone.status();
    assert_eq!(status["state"], "conflicts");
    assert_eq!(status["conflicted_ids"], json!([ID]));

    phone.unlock();

    wait_for("the conflict to settle", || {
        phone.status()["state"] == "synced"
    });
    assert_eq!(phone.markdown(ID), "laptop's later words");
    assert_eq!(phone.titles(), ["Trip", "Trip (conflicted copy)"]);
    // The copy is a local write like any other, so it reaches the laptop.
    laptop.ok("sync_now", json!({}));
    assert_eq!(laptop.titles(), ["Trip", "Trip (conflicted copy)"]);
    assert_eq!(laptop.markdown(ID), "laptop's later words");
}

/// Syncing must never hold the vault's lock state across I/O (see
/// `VaultState`): a slow network would freeze logout, and every command with
/// it. A pass is frozen inside the remote, and the lock state is used
/// meanwhile.
#[test]
fn a_pass_stuck_on_the_network_does_not_hold_up_the_vault() {
    let gated = Gated::new(Arc::new(MemoryRemote::new()));
    let rig = Rig::new(gated.clone());
    rig.unlock();
    rig.connect();

    gated.set_open(false);
    let control = rig.control.clone();
    let pass = thread::spawn(move || control.sync_now());
    wait_for("the pass to start", || rig.status()["state"] == "syncing");

    let (done, finished) = std::sync::mpsc::channel();
    let handle = rig.app.handle().clone();
    thread::spawn(move || {
        let state = handle.state::<VaultState>();
        assert!(crate::vault::get(&state).is_ok());
        crate::vault::clear(&state);
        assert!(crate::vault::get(&state).is_err());
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(5))
        .expect("locking the vault waited for a sync pass");

    gated.set_open(true);
    pass.join().unwrap().unwrap();
}

/// `SyncControl::set_interval` called before a worker exists (before the
/// first `connect`) is picked up by the worker `connect` starts: `install`
/// reads the stored interval under the same `lifecycle` lock `set_interval`
/// stores it under, so a change cannot land in between.
#[test]
fn set_interval_before_connecting_is_picked_up_by_the_worker_that_starts() {
    let rig = Rig::new(Arc::new(MemoryRemote::new()));

    rig.control.set_interval(Some(Duration::from_secs(42)));
    rig.connect();

    assert_eq!(
        rig.control
            .running_worker_timing_for_test()
            .expect("a worker is running")
            .interval,
        Some(Duration::from_secs(42))
    );
}

/// The same, for a worker `resume` starts at launch rather than `connect`.
#[test]
fn set_interval_before_resuming_is_picked_up_by_the_worker_that_starts() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    FileCredentialStore::new(rig.dir.join("config").join("login"))
        .save(&Credential::new("fake", RefreshToken::new("refresh")))
        .unwrap();

    rig.control.set_interval(Some(Duration::from_secs(17)));
    rig.control.resume();

    assert_eq!(
        rig.control
            .running_worker_timing_for_test()
            .expect("a worker is running")
            .interval,
        Some(Duration::from_secs(17))
    );
}

/// The journal half of settings syncs like any other blob, and a conflict on
/// it merges per field (the resolver's `SETTINGS_BLOB` dispatch): two devices
/// each change a different journal field, and both end up with both changes.
#[test]
fn two_devices_changing_different_journal_settings_both_end_up_with_both() {
    let remote = Arc::new(MemoryRemote::new());
    let laptop = Rig::new(remote.clone());
    laptop.unlock();
    laptop.connect();
    let phone = Rig::new(remote.clone());
    phone.connect();
    phone.unlock();

    laptop
        .update_settings(json!({ "time_format": "h24" }))
        .unwrap();
    laptop.ok("sync_now", json!({})); // pushes the laptop's change

    phone
        .update_settings(json!({ "week_starts_on": "monday" }))
        .unwrap();
    // The phone's push loses the precondition race to the laptop's: this
    // pass pulls the laptop's version, conflicts, and merges the two locally
    // (`VaultResolver::resolve` dispatching `SETTINGS_BLOB` to
    // `merge_journal_settings`). The merge write is only marked dirty here;
    // it needs its own pass to actually reach the remote.
    phone.ok("sync_now", json!({}));
    phone.ok("sync_now", json!({})); // pushes the merged result

    laptop.ok("sync_now", json!({})); // pulls the merged result

    let laptop_settings = laptop.get_settings();
    assert_eq!(laptop_settings["time_format"], "h24");
    assert_eq!(laptop_settings["week_starts_on"], "monday");

    let phone_settings = phone.get_settings();
    assert_eq!(phone_settings["time_format"], "h24");
    assert_eq!(phone_settings["week_starts_on"], "monday");
}

// ---- Two journals: probing at connect, and switching to the cloud's. ----

/// Stands in for `vault::take_drained`: counts locks and undos.
#[derive(Default)]
struct FakeLock {
    locks: Arc<AtomicUsize>,
    undos: Arc<AtomicUsize>,
    refuse: AtomicBool,
}

impl FakeLock {
    fn call(&self) -> Result<VaultUndo, String> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err("A save is still running. Try again.".to_owned());
        }
        self.locks.fetch_add(1, Ordering::SeqCst);
        let undos = self.undos.clone();
        Ok(Box::new(move || {
            undos.fetch_add(1, Ordering::SeqCst);
        }))
    }

    fn locks(&self) -> usize {
        self.locks.load(Ordering::SeqCst)
    }

    fn undos(&self) -> usize {
        self.undos.load(Ordering::SeqCst)
    }
}

const KEY: &str = ".vault-key";
const OTHER_ID: &str = "5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a02";

impl Rig {
    fn connect_outcome(&self) -> Value {
        self.ok("connect_remote", json!({ "provider": "fake" }))
    }

    fn adopt(&self, lock: &FakeLock) -> Result<AdoptedJournal, String> {
        self.control.adopt_remote(&|| lock.call())
    }

    fn journal(&self) -> PathBuf {
        self.dir.join("entries")
    }

    fn key_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.journal().join(KEY)).ok()
    }

    fn backups(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("entries.backup-")
            })
            .collect()
    }

    fn pending_is_set(&self) -> bool {
        self.control.pending.lock().unwrap().is_some()
    }

    fn worker_runs(&self) -> bool {
        self.control.config.slot.get().is_some()
    }
}

fn remote_key(remote: &MemoryRemote) -> Vec<u8> {
    remote
        .get(&BlobName::try_from(KEY).unwrap())
        .unwrap()
        .expect("the cloud holds a key")
        .0
}

/// A cloud holding another device's journal, and a device with its own,
/// not connected to anything.
fn cloud_and_local() -> (Arc<MemoryRemote>, Arc<Fake>, Rig) {
    let remote = Arc::new(MemoryRemote::new());
    let cloud_device = Rig::new(remote.clone());
    cloud_device.unlock();
    cloud_device.save(
        ID,
        "Written on the laptop",
        "cloud words",
        "2026-03-01T10:00:00Z",
    );
    cloud_device.connect();
    drop(cloud_device);

    let fake = Fake::new("fake", remote.clone());
    let local = Rig::with_providers(vec![fake.clone()]);
    local.unlock();
    local.save(OTHER_ID, "Mine", "local words", "2026-03-01T11:00:00Z");
    (remote, fake, local)
}

#[test]
fn connecting_to_a_different_journal_saves_nothing() {
    let (_remote, _fake, local) = cloud_and_local();

    assert_eq!(local.connect_outcome(), json!("different_journal"));

    assert_eq!(local.saved_provider(), None);
    assert!(!local.worker_runs());
    assert!(!local.dir.join("sync-state.json").exists());
    assert_eq!(local.status()["state"], "off");
    assert!(local.pending_is_set());
}

#[test]
fn connecting_to_a_different_journal_keeps_the_old_login_running() {
    let (cloud, fake, local) = cloud_and_local();
    let empty = Arc::new(MemoryRemote::new());
    fake.sign_in_as("account-0", empty);
    assert_eq!(local.connect_outcome(), json!("connected"));
    assert_eq!(local.saved_token().as_deref(), Some("refresh-1"));

    fake.sign_in_as("account-2", cloud);
    assert_eq!(local.connect_outcome(), json!("different_journal"));

    assert_eq!(local.saved_token().as_deref(), Some("refresh-1"));
    assert!(local.worker_runs());
    assert_ne!(local.status()["state"], "off");
}

#[test]
fn connecting_to_the_same_journal_connects() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    assert_eq!(rig.connect_outcome(), json!("connected"));
    rig.ok("disconnect_remote", json!({}));
    assert_eq!(rig.connect_outcome(), json!("connected"));
    assert!(rig.worker_runs());
}

#[test]
fn connecting_to_an_empty_cloud_connects() {
    let rig = Rig::new(Arc::new(MemoryRemote::new()));
    rig.unlock();
    assert_eq!(rig.connect_outcome(), json!("connected"));
    assert_eq!(rig.saved_provider().as_deref(), Some("fake"));
}

#[test]
fn a_failed_probe_fails_the_connect_and_saves_nothing() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    remote.fail_next(RemoteError::Transient("offline".into()));

    assert!(rig
        .invoke("connect_remote", json!({ "provider": "fake" }))
        .is_err());

    assert_eq!(rig.saved_provider(), None);
    assert!(!rig.worker_runs());
    assert!(!rig.pending_is_set());
}

#[test]
fn discarding_the_pending_login_revokes_it() {
    let (_remote, fake, local) = cloud_and_local();
    local.connect_outcome();

    local.control.discard_pending_connection().unwrap();

    assert!(!local.pending_is_set());
    wait_for("the revoke", || fake.revoked() == ["refresh-1"]);
}

#[test]
fn a_new_connect_discards_the_pending_login() {
    let (_remote, fake, local) = cloud_and_local();
    local.connect_outcome();
    local.connect_outcome();

    wait_for("the revoke", || fake.revoked() == ["refresh-1"]);
    assert!(
        local.pending_is_set(),
        "the second grant is the pending one"
    );
}

#[test]
fn disconnecting_discards_the_pending_login() {
    let (_remote, fake, local) = cloud_and_local();
    local.connect_outcome();

    local.control.disconnect().unwrap();

    wait_for("the revoke", || fake.revoked() == ["refresh-1"]);
    let lock = FakeLock::default();
    assert_eq!(local.adopt(&lock).unwrap_err(), "Nothing is connected.");
}

#[test]
fn adopting_moves_the_journal_aside_and_pulls_the_clouds() {
    let (remote, _fake, local) = cloud_and_local();
    local.connect_outcome();
    let old_key = local.key_bytes().unwrap();
    let old_entry = std::fs::read(local.journal().join(format!("{OTHER_ID}.json"))).unwrap();
    let lock = FakeLock::default();

    let adopted = local.adopt(&lock).unwrap();

    assert!(adopted.ready);
    assert_eq!(local.key_bytes().unwrap(), remote_key(&remote));
    assert!(local.journal().join(format!("{ID}.json")).exists());
    assert!(!local.journal().join(format!("{OTHER_ID}.json")).exists());
    let backups = local.backups();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        adopted.backup_dir.as_deref(),
        Some(backups[0].display().to_string().as_str())
    );
    assert_eq!(std::fs::read(backups[0].join(KEY)).unwrap(), old_key);
    assert_eq!(
        std::fs::read(backups[0].join(format!("{OTHER_ID}.json"))).unwrap(),
        old_entry
    );
    assert_eq!(local.saved_token().as_deref(), Some("refresh-1"));
    assert!(!local.pending_is_set());
    assert_eq!((lock.locks(), lock.undos()), (1, 0));
    // Nothing of the old journal reached the cloud.
    assert!(!remote_has(&remote, &format!("{OTHER_ID}.json")));
}

#[test]
fn adopting_revokes_the_replaced_login() {
    let (cloud, fake, local) = cloud_and_local();
    fake.sign_in_as("account-0", Arc::new(MemoryRemote::new()));
    local.connect_outcome();
    fake.sign_in_as("account-1", cloud);
    assert_eq!(local.connect_outcome(), json!("different_journal"));

    local.adopt(&FakeLock::default()).unwrap();

    assert_eq!(local.saved_token().as_deref(), Some("refresh-2"));
    wait_for("the revoke", || fake.revoked() == ["refresh-1"]);
}

#[test]
fn adopting_from_the_halted_state_uses_the_saved_login() {
    let (remote, _fake, local) = cloud_and_local();
    // This device connected to an empty cloud, then the cloud got another
    // journal's key.
    let empty = Arc::new(MemoryRemote::new());
    let fake = Fake::new("fake", empty.clone());
    let halted = Rig::with_providers(vec![fake]);
    halted.unlock();
    halted.connect();
    let name = BlobName::try_from(KEY).unwrap();
    let rev = empty.get(&name).unwrap().unwrap().1;
    empty
        .put(&name, &remote_key(&remote), Precondition::Matches(rev))
        .unwrap();
    assert!(halted.control.sync_now().is_err());
    drop(local);

    let lock = FakeLock::default();
    let adopted = halted.adopt(&lock).unwrap();

    assert!(adopted.ready);
    assert_eq!(halted.key_bytes().unwrap(), remote_key(&empty));
    assert_eq!(halted.backups().len(), 1);
    assert_eq!(halted.saved_token().as_deref(), Some("refresh-1"));
    assert!(halted.worker_runs());
}

#[test]
fn adopting_when_the_keys_match_moves_nothing() {
    let (remote, _fake, local) = cloud_and_local();
    local.connect_outcome();
    // The cloud's key becomes this device's.
    let name = BlobName::try_from(KEY).unwrap();
    let rev = remote.get(&name).unwrap().unwrap().1;
    remote
        .put(
            &name,
            &local.key_bytes().unwrap(),
            Precondition::Matches(rev),
        )
        .unwrap();
    let lock = FakeLock::default();

    assert!(local.adopt(&lock).is_err());

    assert!(local.journal().join(KEY).exists());
    assert!(local.backups().is_empty());
    assert_eq!(lock.locks(), 0);
    assert!(local.pending_is_set());
}

#[test]
fn adopting_never_overwrites_an_existing_backup() {
    let (_remote, _fake, local) = cloud_and_local();
    local.connect_outcome();
    let now = chrono::Utc::now();
    for secs in -1..=4 {
        let stamp = (now + chrono::Duration::seconds(secs)).format("%Y%m%dT%H%M%SZ");
        std::fs::create_dir_all(local.dir.join(format!("entries.backup-{stamp}"))).unwrap();
    }
    let before = local.key_bytes().unwrap();
    let lock = FakeLock::default();

    let error = local.adopt(&lock).unwrap_err();

    assert!(error.contains("already exists"), "{error}");
    assert_eq!(local.key_bytes().unwrap(), before);
    assert_eq!(lock.undos(), 1);
    assert!(local.pending_is_set());
    assert_eq!(
        local.saved_provider(),
        None,
        "the pending login was not kept"
    );
}

#[test]
fn a_refused_vault_lock_changes_nothing() {
    let (_remote, _fake, local) = cloud_and_local();
    local.connect_outcome();
    let lock = FakeLock::default();
    lock.refuse.store(true, Ordering::SeqCst);
    let before = local.key_bytes().unwrap();

    assert!(local.adopt(&lock).is_err());

    assert_eq!(local.key_bytes().unwrap(), before);
    assert!(local.backups().is_empty());
    assert_eq!(local.saved_provider(), None);
    assert!(local.pending_is_set());
}

#[test]
fn a_failed_state_erase_moves_nothing() {
    let (_remote, _fake, local) = cloud_and_local();
    local.connect_outcome();
    // A directory with a file in it cannot be removed as the state file.
    let state = local.dir.join("sync-state.json");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("x"), b"x").unwrap();
    let lock = FakeLock::default();

    assert!(local.adopt(&lock).is_err());

    assert!(local.journal().join(KEY).exists());
    assert!(local.backups().is_empty());
    assert_eq!(local.saved_provider(), None);
    assert_eq!(lock.undos(), 1);
    assert!(local.pending_is_set());
}

#[test]
fn a_switch_interrupted_after_saving_the_login_halts_instead_of_deleting() {
    let (remote, _fake, local) = cloud_and_local();
    local.connect_outcome();
    // What step 6 leaves: the new login saved, no state, the old journal.
    let pending = local.control.take_pending().unwrap();
    FileCredentialStore::new(local.dir.join("config").join("login"))
        .save(&pending)
        .unwrap();
    let before = remote_key(&remote);
    let listing = |r: &MemoryRemote| {
        ["", ID, OTHER_ID]
            .map(|id| remote_has(r, &format!("{id}.json")))
            .to_vec()
    };
    let before_listing = listing(&remote);

    local.control.resume();
    assert!(local.control.sync_now().is_err());

    assert_eq!(local.status()["state"], "key_mismatch");
    assert_eq!(remote_key(&remote), before);
    assert_eq!(listing(&remote), before_listing);
}

#[test]
fn adopting_while_offline_reports_not_ready() {
    let (remote, _fake, local) = cloud_and_local();
    local.connect_outcome();
    // The probe's `get` succeeds; the first pass's calls fail.
    remote.fail_after(1, RemoteError::Transient("offline".into()));
    remote.fail_after(0, RemoteError::Transient("offline".into()));

    let adopted = local.adopt(&FakeLock::default()).unwrap();

    assert!(!adopted.ready);
    assert!(adopted.backup_dir.is_some());
    assert_eq!(local.backups().len(), 1);
}

#[test]
fn adopting_with_nothing_connected_fails() {
    let rig = Rig::new(Arc::new(MemoryRemote::new()));
    assert_eq!(
        rig.adopt(&FakeLock::default()).unwrap_err(),
        "Nothing is connected."
    );
}

#[test]
fn moving_a_missing_journal_reports_no_backup() {
    let dir = std::env::temp_dir().join(format!("journal-aside-{}", std::process::id()));
    assert_eq!(
        move_journal_aside(&dir.join("entries"), "20261001T000000Z").unwrap(),
        None
    );
    assert!(!dir.exists());
}

#[test]
fn cloud_has_journal_reports_the_remote_key() {
    let (_remote, _fake, local) = cloud_and_local();
    assert_eq!(
        local.control.cloud_has_journal().unwrap_err(),
        "Nothing is connected."
    );

    // Connected, with the cloud's journal arriving: true.
    local.connect_outcome();
    local.adopt(&FakeLock::default()).unwrap();
    assert!(local.control.cloud_has_journal().unwrap());

    // Connected to a cloud with no key: false.
    let blank = Rig::new(Arc::new(MemoryRemote::new()));
    blank.connect();
    assert!(!blank.control.cloud_has_journal().unwrap());
}
