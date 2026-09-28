//! `get_settings` and `update_settings`: the device half from an in-memory,
//! file-backed copy, and the journal half from the vault.
//!
//! See `.claude/plans/device-and-journal-config.md`, "Commands". Step 0 of
//! `update_settings` holds [`DeviceSettingsState::current`] for the whole
//! call, across both writes: two updates racing (a shortcut rebind and a
//! colour toggle, say) must not read the same starting device state and let
//! the second write erase the first.
//!
//! Unlocked, the patch is applied *inside* [`journal_vault::Vault::update_journal_settings`]'s
//! closure, so the journal half is read, re-validated and written under the
//! vault's own `settings_lock` too. Without that, a resolver merge landing
//! between an outside read of the journal half and this write would be
//! reverted (see the plan's WP2 amendment). Locked, the patch is applied
//! against `JournalSettings::default()`, and a patch naming any journal key
//! is refused with [`crate::vault::LOCKED_ERROR`] before anything is written.
//!
//! A change to `sync_interval_minutes` calls `SyncControl::set_interval`
//! while the device lock is still held, and that call waits on
//! `SyncControl`'s own `lifecycle` lock. So a slow `connect` or `disconnect`
//! (which also holds `lifecycle`) can briefly delay `update_settings` — and,
//! because the device lock is held for the whole command, `get_settings` and
//! any other `update_settings` too. Accepted: `set_interval` only runs when
//! the interval actually changed, `lifecycle` is not held across network I/O
//! (the browser wait in `connect` excepted, which is not on this path), and
//! settings changes are rare and user-initiated.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use journal_api::{DeviceSettings, JournalSettings, Settings};
use journal_settings::{apply_patch, DeviceSettingsFile};
use journal_vault::Vault;
use serde_json::{Map, Value};
use tauri::State;

use crate::blocking;
use crate::sync::SyncControl;
use crate::vault::{lock_state, VaultState, LOCKED_ERROR};

/// Minutes, as `DeviceSettings::sync_interval_minutes` stores them, as the
/// `Duration` the sync worker's `Timing::interval` wants.
pub(crate) fn minutes_to_duration(minutes: u16) -> Duration {
    Duration::from_secs(u64::from(minutes) * 60)
}

/// This device's settings: the file they persist to, and an in-memory copy
/// whose lock also serialises every [`DeviceSettingsState::update`] call.
pub struct DeviceSettingsState {
    file: DeviceSettingsFile,
    current: Mutex<DeviceSettings>,
}

impl DeviceSettingsState {
    /// Loads the device half from `path` (defaults on a missing or corrupt
    /// file; see [`DeviceSettingsFile::load`]).
    pub fn load(path: PathBuf) -> Self {
        let file = DeviceSettingsFile::new(path);
        let current = file.load();
        Self {
            file,
            current: Mutex::new(current),
        }
    }

    fn lock(&self) -> MutexGuard<'_, DeviceSettings> {
        self.current.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A copy of the device half as it stands right now. Used at startup to
    /// build the sync worker's initial `Timing`.
    pub fn snapshot(&self) -> DeviceSettings {
        self.lock().clone()
    }

    /// `get_settings`'s work: the device half from here, the journal half
    /// from `vault` while unlocked, or `JournalSettings::default()` while
    /// locked — nothing on screen while locked needs more than the colour
    /// mode.
    fn get(&self, vault: Option<Arc<Vault>>) -> Result<Settings, String> {
        let device = self.snapshot();
        let journal = match vault {
            Some(vault) => vault.journal_settings().map_err(|e| e.to_string())?,
            None => JournalSettings::default(),
        };
        Ok(Settings { device, journal })
    }

    /// `update_settings`'s work; see the module doc comment for the locking
    /// story.
    fn update(
        &self,
        vault: Option<Arc<Vault>>,
        sync: &SyncControl,
        patch: Map<String, Value>,
    ) -> Result<Settings, String> {
        let mut current = self.lock();
        let device_before = current.clone();

        let (device_patch, journal) = match vault {
            Some(vault) => {
                // A device-only patch goes through `update_journal_settings`
                // too, even though it never changes the journal half: this
                // keeps one code path for "unlocked" rather than a second one
                // that skips the vault call whenever `patch` happens to hold
                // no journal key.
                //
                // Stashed from inside the closure, which is the only place
                // `apply_patch` runs while unlocked: `update_journal_settings`
                // hands back only the journal half.
                let mut stashed: Option<Option<DeviceSettings>> = None;
                let journal = vault
                    .update_journal_settings(|current_journal| {
                        let settings = Settings {
                            device: device_before.clone(),
                            journal: current_journal.clone(),
                        };
                        let patched = apply_patch(&settings, patch)?;
                        stashed = Some(patched.device);
                        Ok(patched.journal)
                    })
                    .map_err(|e| e.to_string())?;
                (stashed.flatten(), journal)
            }
            None => {
                let settings = Settings {
                    device: device_before,
                    journal: JournalSettings::default(),
                };
                let patched = apply_patch(&settings, patch).map_err(|e| e.to_string())?;
                // By key, not by whether the value actually changed: see the
                // plan's "Commands" step 2 for why a patch that sets a
                // journal field to the default it reads as while locked must
                // still be refused.
                if patched.names_journal {
                    return Err(LOCKED_ERROR.to_owned());
                }
                (patched.device, JournalSettings::default())
            }
        };

        if let Some(new_device) = device_patch {
            let interval_changed =
                new_device.sync_interval_minutes != current.sync_interval_minutes;
            self.file.save(&new_device).map_err(|e| e.to_string())?;
            *current = new_device;
            if interval_changed {
                sync.set_interval(current.sync_interval_minutes.map(minutes_to_duration));
            }
        }

        Ok(Settings {
            device: current.clone(),
            journal,
        })
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_settings(
    device: State<'_, Arc<DeviceSettingsState>>,
    state: State<'_, VaultState>,
) -> Result<Settings, String> {
    let device = device.inner().clone();
    let vault = lock_state(&state).clone();
    blocking(move || device.get(vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn update_settings(
    device: State<'_, Arc<DeviceSettingsState>>,
    state: State<'_, VaultState>,
    sync: State<'_, Arc<SyncControl>>,
    patch: Map<String, Value>,
) -> Result<Settings, String> {
    let device = device.inner().clone();
    let vault = lock_state(&state).clone();
    let sync = sync.inner().clone();
    blocking(move || device.update(vault, &sync, patch)).await
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use journal_settings::{ColorMode, TimeFormat, WeekStart};
    use journal_store::BlobStore;
    use secrecy::SecretString;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use tauri::Manager;

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "journal-settings-cmd-{label}-{}-{n}",
            std::process::id()
        ))
    }

    fn state() -> (DeviceSettingsState, PathBuf, PathBuf) {
        let dir = temp_dir("device");
        let path = dir.join("device-settings.json");
        (DeviceSettingsState::load(path.clone()), path, dir)
    }

    /// An unlocked vault over its own throwaway store.
    fn unlocked_vault() -> (Arc<Vault>, PathBuf) {
        let dir = temp_dir("vault");
        let store: Arc<dyn BlobStore> = Arc::new(journal_fs_store::FsStore::new(dir.clone()));
        let vault = Vault::open(store, &SecretString::from("passphrase".to_owned()))
            .expect("a fresh store opens as a new vault");
        (Arc::new(vault), dir)
    }

    /// A `SyncControl` with nothing connected, standing in for the app's real
    /// one in tests that only care whether `update_settings` reaches it.
    fn sync_control() -> (Arc<SyncControl>, PathBuf) {
        let dir = temp_dir("sync");
        std::fs::create_dir_all(&dir).unwrap();
        let control = SyncControl::new(crate::sync::SyncConfig {
            slot: Arc::new(crate::sync::Slot::default()),
            local: Arc::new(journal_fs_store::FsStore::new(dir.join("entries"))),
            vault: Arc::new(|| None),
            events: Arc::new(NoEvents),
            providers: Vec::new(),
            credentials: Arc::new(journal_credential::FileCredentialStore::new(
                dir.join("login"),
            )),
            state_path: dir.join("sync-state.json"),
            open_url: Arc::new(|_| Ok(())),
            timing: crate::sync::worker::Timing::default(),
        })
        .expect("sync control builds");
        (Arc::new(control), dir)
    }

    struct NoEvents;
    impl crate::sync::Events for NoEvents {
        fn status(&self, _: &journal_api::SyncStatusPayload) {}
        fn changed(&self) {}
    }

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(m) => m,
            _ => panic!("expected an object"),
        }
    }

    #[test]
    fn get_settings_while_locked_is_device_plus_journal_defaults() {
        let (state, _path, dir) = self::state();
        let settings = state.get(None).unwrap();
        assert_eq!(settings.device, DeviceSettings::default());
        assert_eq!(settings.journal, JournalSettings::default());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_device_only_patch_works_while_locked_and_persists_to_the_file() {
        let (state, path, dir) = self::state();
        let (sync, sync_dir) = sync_control();

        let settings = state
            .update(None, &sync, map(json!({"color_mode": "dark"})))
            .unwrap();

        assert_eq!(settings.device.color_mode, ColorMode::Dark);
        // A fresh state loading the same file sees the same value.
        let reloaded = DeviceSettingsState::load(path).snapshot();
        assert_eq!(reloaded.color_mode, ColorMode::Dark);

        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }

    #[test]
    fn a_journal_patch_while_locked_answers_vault_is_locked_and_writes_nothing() {
        let (state, path, dir) = self::state();
        let (sync, sync_dir) = sync_control();

        let err = state
            .update(
                None,
                &sync,
                map(json!({"time_format": "h24", "color_mode": "dark"})),
            )
            .unwrap_err();

        assert_eq!(err, LOCKED_ERROR);
        // Nothing written, including the device key in the same patch.
        assert_eq!(state.snapshot(), DeviceSettings::default());
        assert!(!path.exists(), "the device file must not be created");
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }

    #[test]
    fn a_journal_patch_to_the_default_while_locked_is_refused_and_survives_unlock() {
        let (state, _path, dir) = self::state();
        let (sync, sync_dir) = sync_control();
        let (vault, vault_dir) = unlocked_vault();
        vault
            .save_journal_settings(&JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            })
            .unwrap();

        // While locked, the journal half reads as defaults, so this patch
        // looks unchanged: it must still be refused by key.
        let err = state
            .update(None, &sync, map(json!({"time_format": "locale"})))
            .unwrap_err();
        assert_eq!(err, LOCKED_ERROR);

        // h24 is still there once "unlocked" (the vault reachable again).
        let settings = state.get(Some(vault.clone())).unwrap();
        assert_eq!(settings.journal.time_format, TimeFormat::H24);

        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
        std::fs::remove_dir_all(vault_dir).ok();
    }

    #[test]
    fn a_journal_patch_works_while_unlocked() {
        let (state, _path, dir) = self::state();
        let (sync, sync_dir) = sync_control();
        let (vault, vault_dir) = unlocked_vault();

        let settings = state
            .update(
                Some(vault.clone()),
                &sync,
                map(json!({"time_format": "h24"})),
            )
            .unwrap();

        assert_eq!(settings.journal.time_format, TimeFormat::H24);
        assert_eq!(
            vault.journal_settings().unwrap().time_format,
            TimeFormat::H24
        );

        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
        std::fs::remove_dir_all(vault_dir).ok();
    }

    #[test]
    fn an_unknown_key_is_refused_with_its_message() {
        let (state, _path, dir) = self::state();
        let (sync, sync_dir) = sync_control();

        let err = state
            .update(None, &sync, map(json!({"nope": 1})))
            .unwrap_err();

        assert!(err.contains("unknown setting"), "{err}");
        assert!(err.contains("nope"), "{err}");
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }

    #[test]
    fn an_invalid_value_is_refused_with_its_message() {
        let (state, _path, dir) = self::state();
        let (sync, sync_dir) = sync_control();

        let err = state
            .update(None, &sync, map(json!({"idle_lock_minutes": 0})))
            .unwrap_err();

        assert!(err.contains("idle lock"), "{err}");
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }

    /// The refusal messages do not depend on the lock state: `apply_patch`
    /// runs the same way whether the current settings came from the vault or
    /// from `JournalSettings::default()`.
    #[test]
    fn an_unknown_key_is_refused_with_the_same_message_while_unlocked() {
        let (state, _path, dir) = self::state();
        let (sync, sync_dir) = sync_control();
        let (vault, vault_dir) = unlocked_vault();

        let err = state
            .update(Some(vault), &sync, map(json!({"nope": 1})))
            .unwrap_err();

        assert!(err.contains("unknown setting"), "{err}");
        assert!(err.contains("nope"), "{err}");
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
        std::fs::remove_dir_all(vault_dir).ok();
    }

    #[test]
    fn an_invalid_value_is_refused_with_the_same_message_while_unlocked() {
        let (state, _path, dir) = self::state();
        let (sync, sync_dir) = sync_control();
        let (vault, vault_dir) = unlocked_vault();

        let err = state
            .update(Some(vault), &sync, map(json!({"idle_lock_minutes": 0})))
            .unwrap_err();

        assert!(err.contains("idle lock"), "{err}");
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
        std::fs::remove_dir_all(vault_dir).ok();
    }

    #[test]
    fn changing_the_sync_interval_reaches_sync_control() {
        let (state, _path, dir) = self::state();
        let (sync, sync_dir) = sync_control();

        state
            .update(None, &sync, map(json!({"sync_interval_minutes": 5})))
            .unwrap();

        assert_eq!(
            sync.current_timing_for_test().interval,
            Some(Duration::from_secs(5 * 60))
        );
        assert_eq!(sync.set_interval_call_count_for_test(), 1);
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }

    #[test]
    fn set_interval_is_not_called_when_the_interval_is_unchanged() {
        let (state, _path, dir) = self::state();
        let (sync, sync_dir) = sync_control();

        state
            .update(None, &sync, map(json!({"idle_lock_minutes": 15})))
            .unwrap();

        assert_eq!(sync.set_interval_call_count_for_test(), 0);
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }

    /// `DeviceSettingsFile::save` fails (here, because the target path is
    /// itself a directory): the in-memory copy must stay exactly what it was
    /// before the call, so a later `get_settings` does not report a value
    /// that was never actually persisted.
    #[test]
    fn the_in_memory_copy_is_unchanged_when_the_file_save_fails() {
        let dir = temp_dir("save-fails");
        let path = dir.join("device-settings.json");
        // A directory where the file should go: `save`'s rename onto it
        // fails.
        std::fs::create_dir_all(&path).unwrap();
        let state = DeviceSettingsState::load(path);
        let (sync, sync_dir) = sync_control();

        let err = state
            .update(None, &sync, map(json!({"color_mode": "dark"})))
            .unwrap_err();

        assert!(!err.is_empty());
        assert_eq!(state.snapshot(), DeviceSettings::default());
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }

    /// The wire contract over the real IPC path, in the style of
    /// `lib.rs`'s `search_entries_answers_the_query_the_frontend_sends`: a
    /// device-only patch succeeds while locked and comes back flat
    /// snake_case, and a journal-key patch while locked rejects with exactly
    /// `LOCKED_ERROR` — the string `src/utils/errors.ts` matches verbatim.
    #[test]
    fn ipc_update_settings_wire_contract_over_a_locked_vault() {
        use tauri::utils::acl::ExecutionContext;

        fn request(cmd: &str, args: Value) -> tauri::webview::InvokeRequest {
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
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            }
        }

        fn json_body(response: tauri::ipc::InvokeResponseBody) -> Value {
            match response {
                tauri::ipc::InvokeResponseBody::Json(json) => {
                    serde_json::from_str(&json).expect("a command answers with JSON")
                }
                tauri::ipc::InvokeResponseBody::Raw(_) => panic!("expected a JSON response"),
            }
        }

        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        for command in ["get_settings", "update_settings"] {
            context
                .runtime_authority_mut()
                .__allow_command(command.into(), ExecutionContext::Local);
        }
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![get_settings, update_settings])
            .build(context)
            .expect("mock app builds");

        let store_dir = temp_dir("wire-store");
        let store: Arc<dyn BlobStore> = Arc::new(journal_fs_store::FsStore::new(store_dir.clone()));
        app.manage(VaultState {
            store,
            vault: Mutex::new(None),
            generation: std::sync::atomic::AtomicU64::new(0),
        });
        let dir = temp_dir("wire");
        app.manage(Arc::new(DeviceSettingsState::load(
            dir.join("device-settings.json"),
        )));
        let (sync, sync_dir) = sync_control();
        app.manage(sync);

        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");

        // A device-only patch succeeds while locked, and the response is
        // flat snake_case, not `{ device: ..., journal: ... }`.
        let response = json_body(
            tauri::test::get_ipc_response(
                &webview,
                request(
                    "update_settings",
                    json!({ "patch": { "color_mode": "dark" } }),
                ),
            )
            .expect("a device-only patch succeeds while locked"),
        );
        assert_eq!(response["color_mode"], "dark");
        for key in [
            "color_mode",
            "idle_lock_minutes",
            "sync_interval_minutes",
            "shortcuts",
            "week_starts_on",
            "time_format",
            "entry_template",
        ] {
            assert!(
                response.get(key).is_some(),
                "missing flat top-level key `{key}`: {response}"
            );
        }
        assert!(response.get("device").is_none(), "must be flat: {response}");
        assert!(
            response.get("journal").is_none(),
            "must be flat: {response}"
        );

        // A journal-key patch while locked rejects with exactly LOCKED_ERROR.
        let err = tauri::test::get_ipc_response(
            &webview,
            request(
                "update_settings",
                json!({ "patch": { "time_format": "h24" } }),
            ),
        )
        .expect_err("a journal key while locked is refused");
        assert_eq!(err, serde_json::json!(LOCKED_ERROR));

        std::fs::remove_dir_all(store_dir).ok();
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }

    #[test]
    fn two_updates_racing_from_separate_threads_both_survive() {
        let (state, _path, dir) = self::state();
        let (vault, vault_dir) = unlocked_vault();
        let state = Arc::new(state);
        let vault = Some(vault);
        let (sync, sync_dir) = sync_control();

        let a_state = state.clone();
        let a_vault = vault.clone();
        let a_sync = sync.clone();
        let a = thread::spawn(move || {
            a_state
                .update(
                    a_vault,
                    &a_sync,
                    map(json!({"color_mode": "dark", "time_format": "h24"})),
                )
                .unwrap();
        });

        let b_state = state.clone();
        let b_vault = vault.clone();
        let b_sync = sync.clone();
        let b = thread::spawn(move || {
            b_state
                .update(
                    b_vault,
                    &b_sync,
                    map(json!({"idle_lock_minutes": 15, "week_starts_on": "monday"})),
                )
                .unwrap();
        });

        a.join().unwrap();
        b.join().unwrap();

        let settings = state.get(vault.clone()).unwrap();
        assert_eq!(settings.device.color_mode, ColorMode::Dark);
        assert_eq!(settings.device.idle_lock_minutes, Some(15));
        assert_eq!(settings.journal.time_format, TimeFormat::H24);
        assert_eq!(settings.journal.week_starts_on, WeekStart::Monday);

        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(vault_dir).ok();
        std::fs::remove_dir_all(sync_dir).ok();
    }
}
