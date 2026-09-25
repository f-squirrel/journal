#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod export;
mod sync;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use journal_api::{Entry, Header, Id, SearchMatch};
use journal_credential::FileCredentialStore;
use journal_fs_store::FsStore;
use journal_key::Keyring;
use journal_store::BlobStore;
use journal_sync_engine::NotifyingStore;
use journal_vault::Vault;
use secrecy::SecretString;
use sync::SyncControl;
use tauri::{Manager, Runtime, State};
use tauri_plugin_opener::OpenerExt;

/// The blob store the journal lives in, plus the unlocked `Vault` over it,
/// if any. `vault` is `None` until `unlock` succeeds, and is cleared again by
/// `lock`.
///
/// The store outlives the lock state because `has_vault` has to answer before
/// there is any passphrase to open one with. It is the only copy of "where the
/// journal is" in the app — the layers below take it as given (ADR 0009).
///
/// Only the lock state is behind a mutex: the store never changes after setup,
/// so `has_vault` needs no lock at all. The `Vault` is an `Arc` so a command
/// can take a handle to it and drop the guard *before* doing any I/O — a slow
/// `save_entry` must not hold up `lock` (i.e. logout).
///
/// `generation` is what makes that safe for `unlock`, which also runs its work
/// with the guard free: it counts every change of intent about the lock state,
/// so an `unlock` that finishes after a `lock` can tell that it is publishing
/// a vault nobody asked for any more. See [`begin_unlock`] and
/// [`install_vault`].
struct VaultState {
    store: Arc<dyn BlobStore>,
    vault: Mutex<Option<Arc<Vault>>>,
    generation: AtomicU64,
}

const LOCKED_ERROR: &str = "vault is locked";

/// The lock state, recovering the guard from poisoning rather than
/// propagating it.
///
/// A panic while this was held used to wedge the app: every later command saw
/// a `PoisonError` and refused, for the life of the process, to touch a vault
/// it still had the key to. What the mutex guards is one `Option` — there is
/// no half-written invariant a panic could leave behind — so the guard is
/// worth more than the poison flag.
fn lock_state(state: &VaultState) -> MutexGuard<'_, Option<Arc<Vault>>> {
    state.vault.lock().unwrap_or_else(|e| e.into_inner())
}

/// A handle to the unlocked vault, or the locked error.
///
/// Every CRUD command goes through here, and none of them holds the guard: it
/// is dropped with the temporary this returns from, so the crypto that follows
/// runs with the mutex free.
fn vault(state: &VaultState) -> Result<Arc<Vault>, String> {
    lock_state(state)
        .clone()
        .ok_or_else(|| LOCKED_ERROR.to_string())
}

/// Claims a generation for an `unlock` that is about to start.
///
/// Paired with [`install_vault`], which publishes the result only if this is
/// still the current generation when the open finishes.
fn begin_unlock(state: &VaultState) -> u64 {
    state.generation.fetch_add(1, Ordering::SeqCst) + 1
}

/// Publishes a freshly opened vault, unless the intent it was opened under is
/// stale.
///
/// `unlock` spends ~1s in scrypt with the guard free, and `lock` is a separate
/// command that can land inside that window. Installing unconditionally would
/// put the vault back *after* an explicit logout, leaving the session unlocked
/// with the entries one command away.
///
/// A stale generation is an error rather than an `Ok(())`, because the
/// frontend sets `authenticated = true` on success: reporting success over a
/// state this call just decided to leave `None` would produce a signed-in
/// screen where every command answers [`LOCKED_ERROR`]. Which is also the
/// honest message — the vault is locked, because the user locked it. `Ok`
/// therefore means, and only means, that the state now holds the vault this
/// call opened.
fn install_vault(state: &VaultState, generation: u64, opened: Vault) -> Result<(), String> {
    let mut vault = lock_state(state);
    if state.generation.load(Ordering::SeqCst) != generation {
        return Err(LOCKED_ERROR.to_string());
    }
    *vault = Some(Arc::new(opened));
    Ok(())
}

/// Clears the lock state, and invalidates any `unlock` still in flight.
///
/// The bump happens under the guard so that it cannot fall between an
/// `install_vault`'s check and its write.
fn lock_vault(state: &VaultState) {
    let mut vault = lock_state(state);
    state.generation.fetch_add(1, Ordering::SeqCst);
    *vault = None;
}

/// Runs `work` on Tauri's blocking pool, flattening the join error and the
/// work's own error into the one string the bridge can carry.
///
/// Every command's body is a call to this. A non-`async` `#[tauri::command]`
/// runs on the main thread and freezes the webview for its duration; the
/// `(async)` attribute on a sync function moves it onto a tokio *worker*,
/// which it then blocks. Blocking work belongs on the blocking pool — which
/// is also why `journal-vault` and the crates below it stay synchronous: the
/// async boundary belongs here, at the command layer.
async fn blocking<T, E>(work: impl FnOnce() -> Result<T, E> + Send + 'static) -> Result<T, String>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn has_vault(state: State<'_, VaultState>) -> Result<bool, String> {
    let store = state.store.clone();
    blocking(move || Keyring::exists(&*store)).await
}

#[tauri::command]
async fn unlock(
    state: State<'_, VaultState>,
    sync: State<'_, Arc<SyncControl>>,
    passphrase: String,
) -> Result<(), String> {
    let store = state.store.clone();
    let passphrase = SecretString::from(passphrase);

    // The one command that is actually slow (~1s of scrypt). Opened without
    // the lock held, then stored — but only if nothing changed the lock state
    // meanwhile.
    let generation = begin_unlock(&state);
    let opened = blocking(move || Vault::open(store, &passphrase)).await?;

    install_vault(&state, generation, opened)?;
    // A conflict parked while the vault was locked can be settled now.
    sync.kick();
    Ok(())
}

#[tauri::command]
async fn lock(state: State<'_, VaultState>) -> Result<(), String> {
    lock_vault(&state);
    Ok(())
}

#[tauri::command]
async fn list_entries(state: State<'_, VaultState>) -> Result<Vec<Header>, String> {
    let vault = vault(&state)?;
    blocking(move || vault.list_entries()).await
}

#[tauri::command]
async fn get_entry(state: State<'_, VaultState>, id: Id) -> Result<Option<Entry>, String> {
    let vault = vault(&state)?;
    blocking(move || vault.get_entry(&id)).await
}

#[tauri::command]
async fn save_entry(state: State<'_, VaultState>, entry: Entry) -> Result<(), String> {
    let vault = vault(&state)?;
    blocking(move || vault.save_entry(&entry)).await
}

#[tauri::command]
async fn delete_entry(state: State<'_, VaultState>, id: Id) -> Result<(), String> {
    let vault = vault(&state)?;
    blocking(move || vault.delete_entry(&id)).await
}

/// The slowest command in the app: it decrypts every entry in the journal
/// (`Vault::search`, which explains why there is no index). That makes the
/// `blocking` hop every command here makes matter more than anywhere else —
/// on the main thread this would freeze the webview for the length of a
/// keystroke's search.
#[tauri::command]
async fn search_entries(
    state: State<'_, VaultState>,
    query: String,
) -> Result<Vec<SearchMatch>, String> {
    let vault = vault(&state)?;
    blocking(move || vault.search(&query)).await
}

/// Writes every entry to `directory` as a plaintext markdown file
/// (`export::to_directory`, which holds the rules about what folders it will
/// take). The one command that deliberately puts unencrypted journal content on
/// disk, so the frontend says so before it calls this.
///
/// Generic over the runtime only for the `AppHandle`, which is where the app's
/// data directory — the one place an export must not land — is asked for.
#[tauri::command]
async fn export_entries<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, VaultState>,
    directory: String,
) -> Result<usize, String> {
    let vault = vault(&state)?;
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let count =
        blocking(move || export::to_directory(&vault, Path::new(&directory), &app_data)).await?;
    // The count and nothing else: not the folder, not a title. Nothing logged at
    // `Info` may carry the journal, and the log file outlives the export.
    log::info!("exported {count} entries as plaintext");
    Ok(count)
}

/// The app's log sink. Without one installed, `log::warn!` anywhere in the
/// workspace compiles down to nothing — notably the "skipped an entry that
/// would not decrypt" warning in `journal_vault::list_entries`, which is
/// the only signal that an entry silently vanished from the list.
///
/// `tauri-plugin-log` rather than a plain stderr subscriber because two of
/// the three target platforms have no stderr a developer will ever see: it
/// routes `Stderr` to logcat on Android and oslog on iOS.
///
/// The targets replace the plugin's defaults rather than add to them: those
/// are `Stdout` and a log file, so adding `Stderr` on top printed every line
/// twice. The log file is kept on purpose — a desktop app is rarely started
/// from a terminal, and a sync failure reported by a user is only
/// diagnosable from what the app wrote down at the time. The plugin caps it
/// (40 kB, one rotated copy), and nothing logged at `Info` carries a secret:
/// the Dropbox SDK's token-logging target stays below the level.
fn logging_plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_log::{Target, TargetKind};

    tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::Stderr),
            Target::new(TargetKind::LogDir { file_name: None }),
        ])
        .level(log::LevelFilter::Info)
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(logging_plugin())
        .plugin(tauri_plugin_opener::init())
        // The folder picker for `export_entries`; see `capabilities/default.json`
        // for the one command of it the webview may call.
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            has_vault,
            unlock,
            lock,
            list_entries,
            get_entry,
            save_entry,
            delete_entry,
            search_entries,
            export_entries,
            sync::list_providers,
            sync::connect_remote,
            sync::disconnect_remote,
            sync::sync_now,
            sync::sync_status
        ])
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let entries = FsStore::new(data_dir.join("entries"));

            // The vault writes through a store that reports each write to sync,
            // and sync reads and writes the store beneath it: its own pulls are
            // not edits. The slot exists first because the callback reaches the
            // worker through it, and there is no worker until an account is
            // connected.
            let slot = Arc::new(sync::Slot::default());
            let notifying = {
                let slot = slot.clone();
                NotifyingStore::new(entries.clone(), move |name| slot.dirty(name))
            };
            app.manage(VaultState {
                store: Arc::new(notifying),
                vault: Mutex::new(None),
                generation: AtomicU64::new(0),
            });

            let handle = app.handle().clone();
            let opener = app.handle().clone();
            let control = Arc::new(SyncControl::new(sync::SyncConfig {
                slot,
                local: Arc::new(entries),
                vault: Arc::new(move || vault(&handle.state::<VaultState>()).ok()),
                events: Arc::new(sync::TauriEvents(app.handle().clone())),
                // Every provider this build can connect to. Adding one is a line
                // here and an adapter crate; nothing below names a provider.
                providers: vec![Arc::new(sync::Dropbox::from_build())],
                // In the config directory, not the data directory the journal
                // lives in: a credential must never be somewhere that syncs.
                credentials: Arc::new(FileCredentialStore::new(
                    app.path().app_config_dir()?.join("remote-login"),
                )),
                state_path: data_dir.join("sync-state.json"),
                open_url: Arc::new(move |url| {
                    opener
                        .opener()
                        .open_url(url.as_str(), None::<&str>)
                        .map_err(|e| e.to_string())
                }),
                timing: sync::worker::Timing::default(),
            }));
            app.manage(control.clone());
            // Scans every blob against the sync state: not on the main thread.
            std::thread::spawn(move || control.resume());

            if let Some(window) = app.get_webview_window("main") {
                window.maximize()?;
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::panic::AssertUnwindSafe;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tauri::utils::acl::ExecutionContext;

    /// A state whose vault is unlocked, over a store in a directory no other
    /// test in this process will pick. Returned alongside so the caller can
    /// remove it — opening a vault writes the key blob.
    fn unlocked_state() -> (VaultState, PathBuf) {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("journal-cmd-test-{}-{n}", std::process::id()));
        let store: Arc<dyn BlobStore> = Arc::new(FsStore::new(dir.clone()));
        let vault = Vault::open(store.clone(), &SecretString::from("passphrase".to_string()))
            .expect("a fresh store opens as a new vault");
        let state = VaultState {
            store,
            vault: Mutex::new(Some(Arc::new(vault))),
            generation: AtomicU64::new(0),
        };
        (state, dir)
    }

    /// The defect P0-2 names: with no logger installed `log::max_level()` is
    /// `Off`, which is exactly the switch that makes `log::warn!` compile down
    /// to nothing — so entries skipped by `list_entries` vanish with no output.
    /// Must be the only test in this binary that touches the global logger.
    #[test]
    fn logging_is_installed() {
        assert_eq!(log::max_level(), log::LevelFilter::Off);

        let _app = tauri::test::mock_builder()
            .plugin(logging_plugin())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app builds");

        assert_ne!(log::max_level(), log::LevelFilter::Off);
    }

    /// The request the webview sends, with `args` as the argument object
    /// `src/api/*.ts` builds for `invoke(cmd, args)`.
    fn request(cmd: &str, args: serde_json::Value) -> tauri::webview::InvokeRequest {
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            // The local origin, which is what the ACL allowances below match;
            // it is spelled differently per platform.
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        }
    }

    /// A command's successful response, parsed back out of the JSON the
    /// bridge actually carried.
    fn answered(response: Result<tauri::ipc::InvokeResponseBody, serde_json::Value>) -> Value {
        match response.expect("the command succeeded") {
            tauri::ipc::InvokeResponseBody::Json(json) => {
                serde_json::from_str(&json).expect("a command answers with JSON")
            }
            tauri::ipc::InvokeResponseBody::Raw(_) => panic!("no command here answers with bytes"),
        }
    }

    /// A mock context in which `commands` are allowed. A mock context carries
    /// no ACL, and app commands are ACL-gated like any other: without this a
    /// command is rejected before it runs and a test proves nothing about the
    /// vault. `tauri-build` generates the equivalent allowance for the real
    /// binary.
    fn context_allowing(commands: &[&str]) -> tauri::Context<tauri::test::MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        for command in commands {
            context
                .runtime_authority_mut()
                .__allow_command((*command).into(), ExecutionContext::Local);
        }
        context
    }

    /// The locked error crosses the bridge verbatim: `src/api/entries.ts`
    /// shows whatever string a command rejects with, so a command called
    /// before `unlock` has to say so in words the user can act on — and over
    /// the real IPC wire, which is the half `Vault`'s own tests cannot see.
    ///
    /// Search and export are in here with the listing because they are the
    /// commands that read *every* entry: an export answering over a locked vault
    /// would be the whole journal, in plaintext, on disk.
    #[test]
    fn commands_before_unlock_report_the_vault_as_locked() {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![
                list_entries,
                search_entries,
                export_entries
            ])
            .build(context_allowing(&[
                "list_entries",
                "search_entries",
                "export_entries",
            ]))
            .expect("mock app builds");
        app.manage(VaultState {
            store: Arc::new(FsStore::new(
                std::env::temp_dir().join("journal-locked-test"),
            )),
            vault: Mutex::new(None),
            generation: AtomicU64::new(0),
        });
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");

        for request in [
            request("list_entries", serde_json::json!({})),
            request("search_entries", serde_json::json!({ "query": "anything" })),
            request(
                "export_entries",
                serde_json::json!({ "directory": "/nowhere" }),
            ),
        ] {
            let cmd = request.cmd.clone();
            let response = tauri::test::get_ipc_response(&webview, request);
            assert_eq!(
                response.expect_err("a locked vault rejects"),
                serde_json::json!(LOCKED_ERROR),
                "{cmd} answered over a locked vault"
            );
        }
    }

    /// The wire contract of `search_entries`, asserted the way the frontend
    /// actually calls it — `invoke("search_entries", { query })`.
    ///
    /// The argument *name* is the one part of a command nothing else in the
    /// build checks. `tsc` catches a renamed field on a generated type, but
    /// the argument object in `src/api/entries.ts` is hand-written; the
    /// Playwright suite never crosses the bridge at all, because it runs
    /// against the browser fallback. A mismatch here is a failure that
    /// appears only in a packaged app, on the one feature whose whole job is
    /// to read the journal.
    #[test]
    fn search_entries_answers_the_query_the_frontend_sends() {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![search_entries])
            .build(context_allowing(&["search_entries"]))
            .expect("mock app builds");

        let (state, dir) = unlocked_state();
        // A body that holds the query under a title that does not: the case
        // only a backend search can answer, over the real bridge this time.
        vault(&state)
            .expect("the state is unlocked")
            .save_entry(&Entry {
                header: Header {
                    id: Id::new(),
                    title: "Tuesday".to_string(),
                    updated_at: chrono::Utc::now(),
                    entry_date: chrono::Utc::now(),
                },
                markdown: "A long walk by the river.".to_string(),
            })
            .expect("the entry saves");
        app.manage(state);

        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");

        let found = answered(tauri::test::get_ipc_response(
            &webview,
            request("search_entries", serde_json::json!({ "query": "river" })),
        ));

        let matches = found.as_array().expect("search answers with a list");
        assert_eq!(matches.len(), 1, "got {found}");
        assert_eq!(matches[0]["header"]["title"], "Tuesday");
        assert!(matches[0]["snippet"]
            .as_str()
            .expect("a snippet is a string")
            .contains("river"));
        // snake_case on the wire, which is what `src/bindings/SearchMatch.ts`
        // promises the frontend it will find there.
        assert!(
            matches[0]["header"]["entry_date"].is_string(),
            "got {found}"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    /// An empty query is not a request for the whole journal — the property
    /// `Vault::search` holds, checked here because the frontend's debounce
    /// can and does send one as a field is cleared.
    #[test]
    fn search_entries_answers_an_empty_query_with_nothing() {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![search_entries])
            .build(context_allowing(&["search_entries"]))
            .expect("mock app builds");

        let (state, dir) = unlocked_state();
        vault(&state)
            .expect("the state is unlocked")
            .save_entry(&Entry {
                header: Header {
                    id: Id::new(),
                    title: "Tuesday".to_string(),
                    updated_at: chrono::Utc::now(),
                    entry_date: chrono::Utc::now(),
                },
                markdown: "A long walk by the river.".to_string(),
            })
            .expect("the entry saves");
        app.manage(state);

        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");

        let found = answered(tauri::test::get_ipc_response(
            &webview,
            request("search_entries", serde_json::json!({ "query": "  " })),
        ));

        assert_eq!(found.as_array().map(Vec::len), Some(0), "got {found}");

        std::fs::remove_dir_all(dir).ok();
    }

    /// The wire contract of `export_entries`, as `src/api/entries.ts` calls it —
    /// `invoke("export_entries", { directory })` — and the same reason for it as
    /// the search test above: the argument name is hand-written on the frontend
    /// and nothing else in the build checks it.
    #[test]
    fn export_entries_writes_into_the_directory_the_frontend_names() {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![export_entries])
            .build(context_allowing(&["export_entries"]))
            .expect("mock app builds");

        let (state, dir) = unlocked_state();
        vault(&state)
            .expect("the state is unlocked")
            .save_entry(&Entry {
                header: Header {
                    id: Id::new(),
                    title: "Tuesday".to_string(),
                    updated_at: chrono::Utc::now(),
                    entry_date: chrono::Utc::now(),
                },
                markdown: "A long walk by the river.".to_string(),
            })
            .expect("the entry saves");
        app.manage(state);
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");

        // Outside `dir`, which stands in for the app's data.
        let out = std::env::temp_dir().join(format!("journal-export-cmd-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();

        let count = answered(tauri::test::get_ipc_response(
            &webview,
            request(
                "export_entries",
                serde_json::json!({ "directory": out.to_str().unwrap() }),
            ),
        ));

        assert_eq!(count, 1);
        let files: Vec<_> = std::fs::read_dir(&out).unwrap().collect();
        assert_eq!(files.len(), 1);
        let text = std::fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
        assert!(text.contains("A long walk by the river."), "got {text:?}");
        assert!(text.contains("title: \"Tuesday\""), "got {text:?}");

        // A second export into the now non-empty folder is refused, and the
        // refusal reaches the frontend as the message the dialog shows.
        let refused = tauri::test::get_ipc_response(
            &webview,
            request(
                "export_entries",
                serde_json::json!({ "directory": out.to_str().unwrap() }),
            ),
        )
        .expect_err("a non-empty folder is refused");
        assert!(
            refused.as_str().is_some_and(|m| m.contains("not empty")),
            "got {refused}"
        );

        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(out).ok();
    }

    /// R1-2: `unlock` opens the vault with the guard free, so a `lock` — an
    /// explicit logout — can land inside its ~1s of scrypt. Publishing the
    /// result unconditionally would put the vault back afterwards, leaving the
    /// session unlocked with every entry one command away.
    ///
    /// The interleaving itself is not deterministically reproducible, and a
    /// sleep-based test of it would only generate flakes, so what is tested is
    /// the mechanism that closes it: the generation an `unlock` claimed, the
    /// real `lock` path bumping it, and the install refusing.
    #[test]
    fn unlock_does_not_install_a_vault_locked_during_the_open() {
        let (state, dir) = unlocked_state();
        let generation = begin_unlock(&state);
        let opened = Vault::open(
            state.store.clone(),
            &SecretString::from("passphrase".to_string()),
        )
        .expect("the existing vault opens again");

        // The logout that lands while the open above was running.
        lock_vault(&state);

        assert_eq!(
            install_vault(&state, generation, opened),
            Err(LOCKED_ERROR.to_string()),
            "a stale unlock reported success"
        );
        assert!(
            vault(&state).is_err(),
            "the vault was published over an explicit lock"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    /// The other half of the check above, so that it is a check and not a
    /// refusal to ever unlock: an `unlock` nothing interrupted still publishes.
    #[test]
    fn unlock_installs_the_vault_it_opened() {
        let (state, dir) = unlocked_state();
        lock_vault(&state);

        let generation = begin_unlock(&state);
        let opened = Vault::open(
            state.store.clone(),
            &SecretString::from("passphrase".to_string()),
        )
        .expect("the existing vault opens again");

        assert_eq!(install_vault(&state, generation, opened), Ok(()));
        assert!(vault(&state).is_ok(), "the unlock did not take");

        std::fs::remove_dir_all(dir).ok();
    }

    /// A panic while the lock state is held used to wedge the app for the rest
    /// of the process: every later command returned the `PoisonError` string
    /// instead of reading the journal it still had the key to. Recovering the
    /// guard keeps one panic from costing a session.
    ///
    /// Only reproducible under unwinding — the release profile aborts — which
    /// is why this is a `cargo test` guard and not a claim about production.
    #[test]
    fn mutex_poisoning_does_not_wedge_the_vault() {
        let (state, dir) = unlocked_state();

        let panicked = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = state.vault.lock().expect("not poisoned yet");
            panic!("boom");
        }));

        assert!(panicked.is_err());
        assert!(vault(&state).is_ok(), "every later command still works");

        std::fs::remove_dir_all(dir).ok();
    }
}
