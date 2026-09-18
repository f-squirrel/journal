//! The sync wiring end to end, over Tauri's mock runtime and an in-memory
//! remote standing in for Dropbox: the commands, over the real IPC path, on
//! the real vault, worker and engine.
//!
//! Each [`Rig`] is one device — its own app, journal directory and login. Two
//! rigs over one `MemoryRemote` are two devices of one account.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use journal_dropbox::FileTokenStore;
use journal_entry::SyncState;
use journal_fs_store::FsStore;
use journal_remote::{
    ChangePage, Cursor, MemoryRemote, Precondition, RemoteError, RemoteStore, Revision,
};
use journal_store::BlobName;
use journal_sync::NotifyingStore;
use serde_json::{json, Value};
use tauri::test::MockRuntime;
use tauri::utils::acl::ExecutionContext;
use tauri::Manager;

use super::*;
use crate::VaultState;

const PASSPHRASE: &str = "correct horse";
const COMMANDS: [&str; 11] = [
    "has_vault",
    "unlock",
    "lock",
    "list_entries",
    "get_entry",
    "save_entry",
    "delete_entry",
    "connect_dropbox",
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
/// and the remote is whatever it was given.
struct Fake {
    remote: Arc<dyn RemoteStore>,
}

impl Provider for Fake {
    fn begin(&self, redirect_uri: &str) -> Result<Connection, String> {
        let mut url = Url::parse("https://provider.example/authorize").unwrap();
        url.query_pairs_mut()
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", "fake-state");
        Ok(Connection {
            url,
            state: "fake-state".to_owned(),
            redeem: Box::new(|code| match code {
                "the-code" => Ok(RefreshToken::new("refresh")),
                other => Err(format!("unexpected code {other}")),
            }),
        })
    }

    fn remote(&self, _: &RefreshToken) -> Result<Arc<dyn RemoteStore>, String> {
        Ok(self.remote.clone())
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
        interval: Duration::from_secs(3600),
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
        Self::with_provider(Arc::new(Fake { remote }))
    }

    fn with_provider(provider: Arc<dyn Provider>) -> Self {
        Self::build(provider, timing())
    }

    fn build(provider: Arc<dyn Provider>, timing: Timing) -> Self {
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
                crate::has_vault,
                crate::unlock,
                crate::lock,
                crate::list_entries,
                crate::get_entry,
                crate::save_entry,
                crate::delete_entry,
                connect_dropbox,
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
        let handle = app.handle().clone();
        let events = Arc::new(Recorder::default());
        let control = Arc::new(SyncControl::new(SyncConfig {
            slot,
            local: Arc::new(entries),
            vault: Arc::new(move || crate::vault(&handle.state::<VaultState>()).ok()),
            events: events.clone(),
            provider,
            tokens: Arc::new(FileTokenStore::new(dir.join("config").join("token"))),
            state_path: dir.join("sync-state.json"),
            open_url: Arc::new(browser),
            redirect_port: 0,
            timing,
        }));
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

    fn save(&self, id: &str, title: &str, markdown: &str, updated_at: &str) {
        self.ok(
            "save_entry",
            json!({ "entry": {
                "id": id, "title": title, "markdown": markdown, "updated_at": updated_at,
            }}),
        );
    }

    fn status(&self) -> Value {
        self.ok("sync_status", json!({}))
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

    /// Connects, over the loopback and the fake browser.
    fn connect(&self) {
        self.ok("connect_dropbox", json!({}));
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
    let rig = Rig::with_provider(Arc::new(Dropbox { app_key: None }));

    let error = rig.invoke("connect_dropbox", json!({})).unwrap_err();

    assert!(
        error.as_str().unwrap().contains("DROPBOX_APP_KEY"),
        "{error}"
    );
    assert_eq!(rig.status()["state"], "off");
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
    let token = rig.dir.join("config").join("token");
    assert!(token.exists());
    assert!(rig.dir.join("sync-state.json").exists());

    rig.ok("disconnect_remote", json!({}));

    assert_eq!(rig.status()["state"], "off");
    assert!(!token.exists(), "the credential is gone");
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

#[test]
fn an_account_connected_in_an_earlier_run_resumes() {
    let remote = Arc::new(MemoryRemote::new());
    let rig = Rig::new(remote.clone());
    rig.unlock();
    rig.save(ID, "Today", "words", "2026-03-01T10:00:00Z");
    FileTokenStore::new(rig.dir.join("config").join("token"))
        .save(&RefreshToken::new("refresh"))
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
        assert!(crate::vault(&state).is_ok());
        crate::lock_vault(&state);
        assert!(crate::vault(&state).is_err());
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(5))
        .expect("locking the vault waited for a sync pass");

    gated.set_open(true);
    pass.join().unwrap().unwrap();
}
