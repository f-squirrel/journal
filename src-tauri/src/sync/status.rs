//! What the frontend is told about sync: the derivation of the one-word
//! `state`, the payload around it, and the throttle that keeps a burst of
//! engine changes from becoming a burst of events.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use journal_entry::{SyncState, SyncStatusPayload};
use journal_sync::{SyncErrorKind, SyncStatus};
use journal_vault::entry_id;

use super::Events;

/// The state the rail's button shows: the first rule that matches.
///
/// The order is the design. What needs the user (a key mismatch, a login that
/// no longer works) outranks what is merely happening (`syncing`), which
/// outranks a failure that will pass by itself, which outranks the two
/// bookkeeping states — and `synced` is what is left when none applies.
pub fn derive_state(connected: bool, status: &SyncStatus) -> SyncState {
    use SyncErrorKind as Kind;

    if !connected {
        return SyncState::Off;
    }
    match status.last_error {
        Some(Kind::KeyMismatch) => return SyncState::KeyMismatch,
        Some(Kind::Unauthorized) => return SyncState::Reauth,
        _ => {}
    }
    if status.running {
        return SyncState::Syncing;
    }
    match status.last_error {
        Some(Kind::Offline | Kind::RateLimited) => return SyncState::Offline,
        Some(Kind::Other) => return SyncState::Error,
        _ => {}
    }
    if !status.conflicted.is_empty() {
        SyncState::Conflicts
    } else if !status.pending.is_empty() {
        SyncState::Pending
    } else {
        SyncState::Synced
    }
}

/// The payload for the frontend. `provider` is the connected provider's name,
/// or `None` when nothing is connected.
///
/// `detail` is the failing pass's own message, when the worker has one. Blob
/// names become entry ids here, through the vault's own naming: the engine
/// knows names only, and a pending blob that is not an entry is counted, not
/// named.
pub fn payload(
    provider: Option<&str>,
    status: &SyncStatus,
    detail: Option<&str>,
) -> SyncStatusPayload {
    let ids = |names: &std::collections::BTreeSet<_>| -> Vec<_> {
        names.iter().filter_map(entry_id).collect()
    };
    let pending_ids = ids(&status.pending);
    let pending_other = status.pending.len() - pending_ids.len();

    SyncStatusPayload {
        state: derive_state(provider.is_some(), status),
        provider: provider.map(str::to_owned),
        pending_ids,
        conflicted_ids: ids(&status.conflicted),
        pending_other: u32::try_from(pending_other).unwrap_or(u32::MAX),
        last_synced_at: status.last_success.map(DateTime::<Utc>::from),
        message: status.last_error.map(|kind| {
            detail.map_or_else(
                || default_message(provider.unwrap_or("the remote"), kind),
                str::to_owned,
            )
        }),
    }
}

/// Said when a pass has failed but the worker has not recorded why yet.
fn default_message(provider: &str, kind: SyncErrorKind) -> String {
    match kind {
        SyncErrorKind::Offline => format!("Could not reach {provider}."),
        SyncErrorKind::RateLimited => format!("{provider} asked us to slow down."),
        SyncErrorKind::Unauthorized => {
            format!("{provider} no longer accepts this login. Connect again.")
        }
        SyncErrorKind::KeyMismatch => {
            format!("This device and {provider} hold different journal keys.")
        }
        SyncErrorKind::Other => "Sync failed.".to_owned(),
    }
}

/// Sends `sync://status` at most once per `interval`, always with the latest
/// status.
///
/// The engine reports every change — each blob marked, each blob pushed — and
/// several arrive within a millisecond. A poke wakes a thread that reads the
/// status *then*, not when the poke was made, sends it, and sleeps out the
/// interval; pokes that land meanwhile collapse into one more send after it.
/// The first send after a quiet spell is immediate.
pub struct Coalescer {
    tx: mpsc::Sender<()>,
}

impl Coalescer {
    pub fn new(
        interval: Duration,
        snapshot: impl Fn() -> SyncStatusPayload + Send + 'static,
        events: Arc<dyn Events>,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<()>();
        thread::Builder::new()
            .name("sync-status".to_owned())
            .spawn(move || {
                // Ends when the last sender is dropped, i.e. with the app.
                while rx.recv().is_ok() {
                    while rx.try_recv().is_ok() {}
                    events.status(&snapshot());
                    thread::sleep(interval);
                }
            })
            .expect("the OS can start a thread");
        Self { tx }
    }

    /// A handle that pokes, for callbacks that outlive a borrow of `self`. A
    /// send fails only once the thread is gone, at shutdown.
    pub fn poker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let tx = self.tx.clone();
        Arc::new(move || {
            let _ = tx.send(());
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    use std::time::{Instant, SystemTime};

    use journal_entry::Id;
    use journal_store::BlobName;

    use super::*;

    fn name(s: &str) -> BlobName {
        BlobName::try_from(s).unwrap()
    }

    fn entry_name(id: &Id) -> BlobName {
        name(&format!("{id}.json"))
    }

    fn blobs(names: &[BlobName]) -> BTreeSet<BlobName> {
        names.iter().cloned().collect()
    }

    /// A status with everything that could matter turned on, so that each test
    /// below turns *off* what it is not about.
    fn busy() -> SyncStatus {
        SyncStatus {
            pending: blobs(&[name("a.json")]),
            conflicted: blobs(&[name("a.json")]),
            running: true,
            last_success: None,
            last_error: None,
        }
    }

    fn state(connected: bool, status: &SyncStatus) -> SyncState {
        derive_state(connected, status)
    }

    #[test]
    fn not_connected_is_off_whatever_else_is_true() {
        let mut status = busy();
        status.last_error = Some(SyncErrorKind::KeyMismatch);
        assert_eq!(state(false, &status), SyncState::Off);
    }

    #[test]
    fn a_key_mismatch_outranks_everything_connected() {
        let mut status = busy();
        status.last_error = Some(SyncErrorKind::KeyMismatch);
        assert_eq!(state(true, &status), SyncState::KeyMismatch);
    }

    #[test]
    fn revoked_credentials_outrank_a_running_pass() {
        let mut status = busy();
        status.last_error = Some(SyncErrorKind::Unauthorized);
        assert_eq!(state(true, &status), SyncState::Reauth);
    }

    #[test]
    fn a_running_pass_outranks_a_transient_failure_and_the_marks() {
        let mut status = busy();
        status.last_error = Some(SyncErrorKind::Offline);
        assert_eq!(state(true, &status), SyncState::Syncing);
    }

    #[test]
    fn offline_and_rate_limited_are_both_offline() {
        for kind in [SyncErrorKind::Offline, SyncErrorKind::RateLimited] {
            let mut status = busy();
            status.running = false;
            status.last_error = Some(kind);
            assert_eq!(state(true, &status), SyncState::Offline, "{kind:?}");
        }
    }

    #[test]
    fn any_other_failure_is_an_error_and_outranks_the_marks() {
        let mut status = busy();
        status.running = false;
        status.last_error = Some(SyncErrorKind::Other);
        assert_eq!(state(true, &status), SyncState::Error);
    }

    #[test]
    fn a_parked_conflict_outranks_pending() {
        let mut status = busy();
        status.running = false;
        assert_eq!(state(true, &status), SyncState::Conflicts);
    }

    #[test]
    fn pending_alone_is_pending() {
        let mut status = busy();
        status.running = false;
        status.conflicted.clear();
        assert_eq!(state(true, &status), SyncState::Pending);
    }

    #[test]
    fn nothing_outstanding_is_synced() {
        assert_eq!(state(true, &SyncStatus::default()), SyncState::Synced);
    }

    #[test]
    fn the_payload_names_entries_and_counts_the_rest() {
        let (a, b) = (Id::new(), Id::new());
        let status = SyncStatus {
            pending: blobs(&[entry_name(&a), name(".vault-key"), name("stray.txt")]),
            conflicted: blobs(&[entry_name(&b)]),
            last_success: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)),
            ..SyncStatus::default()
        };

        let payload = payload(Some("Dropbox"), &status, None);

        assert_eq!(payload.pending_ids, [a]);
        assert_eq!(payload.conflicted_ids, [b]);
        assert_eq!(payload.pending_other, 2);
        assert_eq!(
            payload.last_synced_at,
            Some("2027-01-15T08:00:00Z".parse().unwrap())
        );
        assert_eq!(payload.message, None);
    }

    #[test]
    fn the_ids_are_sent_in_every_state() {
        let a = Id::new();
        let status = SyncStatus {
            pending: blobs(&[entry_name(&a)]),
            last_error: Some(SyncErrorKind::Offline),
            ..SyncStatus::default()
        };

        let payload = payload(Some("Dropbox"), &status, Some("no route to host"));

        assert_eq!(payload.state, SyncState::Offline);
        assert_eq!(payload.pending_ids, [a]);
        assert_eq!(payload.message.as_deref(), Some("no route to host"));
    }

    #[test]
    fn a_failure_with_no_recorded_detail_still_has_words() {
        let status = SyncStatus {
            last_error: Some(SyncErrorKind::Unauthorized),
            ..SyncStatus::default()
        };
        assert!(payload(Some("Dropbox"), &status, None).message.is_some());
        // And a success carries no message, even if a stale detail is around.
        assert_eq!(
            payload(Some("Dropbox"), &SyncStatus::default(), Some("old")).message,
            None
        );
    }

    /// A missing permission halts sync like a revoked login, but its words are
    /// the error's own: they name what to turn on before reconnecting, which
    /// "connect again" alone would not fix.
    #[test]
    fn a_missing_permission_is_reauth_and_names_the_permission() {
        let error = journal_sync::SyncError::from(journal_remote::RemoteError::MissingPermission(
            "files.content.write".to_owned(),
        ));
        let status = SyncStatus {
            last_error: Some(error.kind()),
            ..SyncStatus::default()
        };

        let sent = payload(Some("Dropbox"), &status, Some(&error.to_string()));

        assert_eq!(sent.state, SyncState::Reauth);
        let message = sent.message.unwrap();
        assert!(message.contains("`files.content.write`"), "{message}");
        assert!(message.contains("connect again"), "{message}");
    }

    #[test]
    fn the_default_words_name_the_connected_provider() {
        let status = SyncStatus {
            last_error: Some(SyncErrorKind::Offline),
            ..SyncStatus::default()
        };

        let message = payload(Some("Google Drive"), &status, None).message;

        assert_eq!(message.as_deref(), Some("Could not reach Google Drive."));
    }

    struct Recorder(Mutex<Vec<(Instant, SyncStatusPayload)>>);

    impl Events for Recorder {
        fn status(&self, payload: &SyncStatusPayload) {
            self.0
                .lock()
                .unwrap()
                .push((Instant::now(), payload.clone()));
        }
        fn changed(&self) {}
    }

    /// A burst of pokes is a couple of events, not one per poke, and the last
    /// event carries the last status.
    #[test]
    fn a_burst_of_pokes_sends_the_latest_status_a_few_times() {
        let recorder = Arc::new(Recorder(Mutex::default()));
        let pending = Arc::new(Mutex::new(0u32));
        let coalescer = {
            let pending = pending.clone();
            Coalescer::new(
                Duration::from_millis(100),
                move || SyncStatusPayload {
                    pending_other: *pending.lock().unwrap(),
                    ..payload(Some("Dropbox"), &SyncStatus::default(), None)
                },
                recorder.clone(),
            )
        };

        let poke = coalescer.poker();
        for n in 1..=50 {
            *pending.lock().unwrap() = n;
            poke();
        }
        thread::sleep(Duration::from_millis(400));

        let seen = recorder.0.lock().unwrap();
        assert!(
            (1..=3).contains(&seen.len()),
            "50 pokes made {} events",
            seen.len()
        );
        assert_eq!(seen.last().unwrap().1.pending_other, 50);
        for pair in seen.windows(2) {
            assert!(pair[1].0 - pair[0].0 >= Duration::from_millis(90));
        }
    }
}
