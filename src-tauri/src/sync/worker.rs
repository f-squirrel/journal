//! The background thread that owns a [`SyncEngine`] and decides when it runs.
//!
//! The engine runs one pass when asked and returns a typed error; *when* to ask
//! and what a failure means for the next ask is this file's job. What it does:
//!
//! - a pass at startup, then one every `interval`;
//! - a pass shortly after a save, debounced so typing is one upload;
//! - a pass on demand, ignoring the schedule (`sync_now`);
//! - after a failure that will pass by itself, exponential backoff;
//! - after one that will not (a revoked login, two different keys), no more
//!   passes of its own accord — only an explicit `sync_now` tries again.
//!
//! The thread is a plain `std::thread`, not a tokio worker (D8): a pass blocks
//! on the network and nothing else waits on it.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use journal_remote::RemoteError;
use journal_store::BlobName;
use journal_sync_engine::{SyncEngine, SyncError, SyncErrorKind, SyncReport};

use super::resolver::VaultResolver;
use super::Events;

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How long after a save the pass waits, if nothing else is saved.
    pub debounce: Duration,
    /// The longest a steady stream of saves can postpone a pass.
    pub max_debounce: Duration,
    /// Between passes when nothing prompts one, or `None` for no periodic
    /// pass: sync still runs at startup, after a save, and on "Sync now"
    /// (device settings' `sync_interval_minutes`, D8).
    pub interval: Option<Duration>,
    /// The wait after a first failure; doubled for each further one.
    pub backoff_base: Duration,
    pub backoff_max: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            debounce: Duration::from_secs(2),
            max_debounce: Duration::from_secs(10),
            // A provider's push channel (Dropbox's `list_folder/longpoll`)
            // would make this a push rather than a poll; a later
            // optimisation. Matches `DeviceSettings::default()`'s
            // `sync_interval_minutes` (1 minute).
            interval: Some(Duration::from_secs(60)),
            backoff_base: Duration::from_secs(5),
            backoff_max: Duration::from_secs(300),
        }
    }
}

/// When the next pass is due. Pure, so the policy is tested without a clock.
pub(super) struct Schedule {
    timing: Timing,
    /// The periodic pass. Starts `Some(now)`, which is the startup pass.
    /// `None` while `timing.interval` is `None`: no periodic pass, only the
    /// startup one, saves, "Sync now" and a failure's own backoff retry.
    timer: Option<Instant>,
    /// A save is waiting to be uploaded: when, and the point past which more
    /// saves stop postponing it.
    dirty: Option<Dirty>,
    /// Nothing runs before this, whatever else is due: the backoff.
    not_before: Instant,
    failures: u32,
    /// The last failure was one retrying cannot fix.
    halted: bool,
}

#[derive(Clone, Copy)]
struct Dirty {
    due: Instant,
    cap: Instant,
}

impl Schedule {
    pub fn new(timing: Timing, now: Instant) -> Self {
        Self {
            timing,
            timer: Some(now),
            dirty: None,
            not_before: now,
            failures: 0,
            halted: false,
        }
    }

    /// When to run next, or `None` to wait for a message.
    pub fn deadline(&self) -> Option<Instant> {
        if self.halted {
            return None;
        }
        let due = match (self.dirty, self.timer) {
            (Some(dirty), Some(timer)) => Some(dirty.due.min(timer)),
            (Some(dirty), None) => Some(dirty.due),
            (None, timer) => timer,
        };
        due.map(|due| due.max(self.not_before))
    }

    /// Stores a new polling interval. Reschedules the periodic pass to
    /// `now + interval` (or clears it, for `None`) only while no backoff
    /// retry is pending — otherwise this would cancel that retry (`None`) or
    /// bring it forward past the backoff — and only while the current timer is not
    /// already due: a settings change landing at (or after) the moment a pass, such as the
    /// startup one, was already due must not cancel or delay it.
    pub fn set_interval(&mut self, interval: Option<Duration>, now: Instant) {
        self.timing.interval = interval;
        let already_due = self.timer.is_some_and(|timer| timer <= now);
        if self.failures == 0 && !already_due {
            self.timer = interval.map(|interval| now + interval);
        }
    }

    /// A blob was written. Each save moves the pass back by `debounce`, but
    /// never past `max_debounce` from the first.
    pub fn saved(&mut self, now: Instant) {
        let cap = self.dirty.map_or(now + self.timing.max_debounce, |d| d.cap);
        self.dirty = Some(Dirty {
            due: (now + self.timing.debounce).min(cap),
            cap,
        });
    }

    /// Run as soon as the backoff allows, skipping the debounce.
    pub fn soon(&mut self, now: Instant) {
        self.dirty = Some(Dirty { due: now, cap: now });
    }

    pub fn succeeded(&mut self, now: Instant) {
        self.failures = 0;
        self.halted = false;
        self.not_before = now;
        self.timer = self.timing.interval.map(|interval| now + interval);
        self.dirty = None;
    }

    /// A failed pass still schedules its own backoff retry even when
    /// `timing.interval` is `None`: only a *successful* pass leaves no
    /// periodic deadline (D8).
    pub fn failed(&mut self, now: Instant, error: &SyncError) {
        self.dirty = None;
        if matches!(
            error.kind(),
            SyncErrorKind::Unauthorized | SyncErrorKind::KeyMismatch
        ) {
            self.halted = true;
            return;
        }
        self.failures += 1;
        let mut delay = self.backoff();
        if let SyncError::Remote(RemoteError::RateLimited { retry_after }) = error {
            delay = delay.max(*retry_after);
        }
        self.not_before = now + delay;
        self.timer = Some(now + delay);
    }

    fn backoff(&self) -> Duration {
        let doublings = self.failures.saturating_sub(1).min(16);
        self.timing
            .backoff_base
            .saturating_mul(1 << doublings)
            .min(self.timing.backoff_max)
    }
}

enum Msg {
    /// A blob was written: sync after the debounce.
    Saved,
    /// Sync soon, without the debounce (the vault was just unlocked).
    Soon,
    /// Sync now, whatever the schedule says, and report how it went.
    Now(mpsc::Sender<Result<(), String>>),
    /// The device settings' `sync_interval_minutes` changed.
    Interval(Option<Duration>),
    Stop,
}

/// A running sync engine and the thread that drives it.
pub struct Worker {
    engine: Arc<SyncEngine>,
    tx: mpsc::Sender<Msg>,
    thread: Mutex<Option<JoinHandle<()>>>,
    /// Why the last pass failed, in the error's own words. Read for the status
    /// message; the engine keeps only the kind.
    detail: Arc<Mutex<Option<String>>>,
    /// The provider's id and name, for the words the status says about it.
    provider: (&'static str, &'static str),
    /// The `Timing` this worker was started with — the schedule itself runs
    /// on its own thread and is not otherwise observable, so tests that check
    /// `install` read a freshly stored interval before starting a worker
    /// (rather than a stale default) look here.
    #[cfg(test)]
    started_with: Timing,
}

struct Context {
    engine: Arc<SyncEngine>,
    resolver: Arc<VaultResolver>,
    events: Arc<dyn Events>,
    detail: Arc<Mutex<Option<String>>>,
    /// Asks for a fresh status event; the message above is not in the engine's
    /// status, so a change to it is announced here.
    poke: Arc<dyn Fn() + Send + Sync>,
}

fn pass_changed_entries(result: &Result<SyncReport, SyncError>, resolved: usize) -> bool {
    resolved > 0
        || result
            .as_ref()
            .is_ok_and(|report| report.pulled + report.deleted > 0)
}

impl Worker {
    pub fn start(
        engine: Arc<SyncEngine>,
        resolver: Arc<VaultResolver>,
        events: Arc<dyn Events>,
        timing: Timing,
        poke: Arc<dyn Fn() + Send + Sync>,
        provider: (&'static str, &'static str),
    ) -> std::io::Result<Arc<Self>> {
        let (tx, rx) = mpsc::channel();
        let detail = Arc::new(Mutex::new(None));
        let context = Context {
            engine: engine.clone(),
            resolver,
            events,
            detail: detail.clone(),
            poke,
        };
        let thread = thread::Builder::new()
            .name("sync-worker".to_owned())
            .spawn(move || context.run(Schedule::new(timing, Instant::now()), rx))?;
        Ok(Arc::new(Self {
            engine,
            tx,
            thread: Mutex::new(Some(thread)),
            detail,
            provider,
            #[cfg(test)]
            started_with: timing,
        }))
    }

    /// The `Timing` this worker was started with. Test-only: see the field's
    /// doc comment.
    #[cfg(test)]
    pub(crate) fn started_with(&self) -> Timing {
        self.started_with
    }

    pub fn engine(&self) -> &SyncEngine {
        &self.engine
    }

    /// The id and name of the provider this worker syncs with.
    pub fn provider(&self) -> (&'static str, &'static str) {
        self.provider
    }

    pub fn detail(&self) -> Option<String> {
        self.detail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A blob was written locally. Marks it pending at once, so the UI reacts
    /// to the save, and schedules the upload.
    pub fn saved(&self, name: &BlobName) {
        self.engine.mark_dirty(name);
        let _ = self.tx.send(Msg::Saved);
    }

    /// Sync soon. Does nothing to a worker that has halted for a reason only
    /// the user can fix.
    pub fn soon(&self) {
        let _ = self.tx.send(Msg::Soon);
    }

    /// The device settings' `sync_interval_minutes` changed. Forwarded to the
    /// running schedule, which leaves a pending backoff retry alone (see
    /// `Schedule::set_interval`).
    pub fn set_interval(&self, interval: Option<Duration>) {
        let _ = self.tx.send(Msg::Interval(interval));
    }

    /// Runs a pass now and waits for it, whatever the schedule says. This is
    /// also how a halted worker is told to try again.
    pub fn sync_now(&self) -> Result<(), String> {
        let (reply, answer) = mpsc::channel();
        self.tx
            .send(Msg::Now(reply))
            .map_err(|_| "sync has stopped".to_owned())?;
        answer
            .recv()
            .unwrap_or_else(|_| Err("sync has stopped".to_owned()))
    }

    /// Stops the thread and waits for it, which is after the pass in progress,
    /// if any. Must not be called with a lock held that the pass could want:
    /// the resolver saves through the same store as the app.
    pub fn stop(&self) {
        let _ = self.tx.send(Msg::Stop);
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

impl Context {
    fn run(self, mut schedule: Schedule, rx: mpsc::Receiver<Msg>) {
        loop {
            let received = match schedule.deadline() {
                Some(due) => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Msg::Saved) => schedule.saved(Instant::now()),
                Ok(Msg::Soon) => schedule.soon(Instant::now()),
                Ok(Msg::Interval(interval)) => schedule.set_interval(interval, Instant::now()),
                Ok(Msg::Now(reply)) => {
                    let _ = reply.send(self.pass(&mut schedule));
                }
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.pass(&mut schedule);
                }
            }
        }
    }

    fn pass(&self, schedule: &mut Schedule) -> Result<(), String> {
        let result = self.engine.sync_once(&*self.resolver);
        let resolved = self.resolver.take_resolved();
        if pass_changed_entries(&result, resolved) {
            self.events.changed();
        }
        let now = Instant::now();
        let outcome = match &result {
            Ok(_) => {
                schedule.succeeded(now);
                *self.detail.lock().unwrap_or_else(PoisonError::into_inner) = None;
                Ok(())
            }
            Err(e) => {
                schedule.failed(now, e);
                let message = e.to_string();
                *self.detail.lock().unwrap_or_else(PoisonError::into_inner) = Some(message.clone());
                Err(message)
            }
        };
        (self.poke)();
        outcome
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    fn timing() -> Timing {
        Timing {
            debounce: 2 * S,
            max_debounce: 10 * S,
            interval: Some(60 * S),
            backoff_base: 5 * S,
            backoff_max: 40 * S,
        }
    }

    fn transient() -> SyncError {
        RemoteError::Transient("down".into()).into()
    }

    #[test]
    fn changed_notification_covers_resolutions_and_successful_disk_changes() {
        let failed = Err(transient());
        assert!(pass_changed_entries(&failed, 1));
        assert!(!pass_changed_entries(&failed, 0));

        let pulled = Ok(SyncReport {
            pulled: 1,
            ..Default::default()
        });
        assert!(pass_changed_entries(&pulled, 0));

        let deleted = Ok(SyncReport {
            deleted: 1,
            ..Default::default()
        });
        assert!(pass_changed_entries(&deleted, 0));
        assert!(!pass_changed_entries(&Ok(SyncReport::default()), 0));
    }

    fn schedule() -> (Schedule, Instant) {
        let start = Instant::now();
        (Schedule::new(timing(), start), start)
    }

    #[test]
    fn the_first_pass_is_due_at_startup() {
        let (schedule, start) = schedule();
        assert_eq!(schedule.deadline(), Some(start));
    }

    #[test]
    fn after_a_pass_the_next_is_one_interval_away() {
        let (mut schedule, start) = schedule();
        schedule.succeeded(start);
        assert_eq!(schedule.deadline(), Some(start + 60 * S));
    }

    #[test]
    fn a_save_is_uploaded_after_the_debounce_and_each_save_pushes_it_back() {
        let (mut schedule, start) = schedule();
        schedule.succeeded(start);

        schedule.saved(start + 10 * S);
        assert_eq!(schedule.deadline(), Some(start + 12 * S));
        schedule.saved(start + 11 * S);
        assert_eq!(schedule.deadline(), Some(start + 13 * S));
    }

    #[test]
    fn a_steady_stream_of_saves_cannot_postpone_the_pass_forever() {
        let (mut schedule, start) = schedule();
        schedule.succeeded(start);

        for second in 1..=30 {
            schedule.saved(start + second * S);
        }

        // The first save was at 1s and the cap is 10s after it.
        assert_eq!(schedule.deadline(), Some(start + 11 * S));
    }

    #[test]
    fn soon_skips_the_debounce() {
        let (mut schedule, start) = schedule();
        schedule.succeeded(start);
        schedule.soon(start + 5 * S);
        assert_eq!(schedule.deadline(), Some(start + 5 * S));
    }

    #[test]
    fn a_failure_backs_off_exponentially_up_to_a_cap() {
        let (mut schedule, start) = schedule();
        let mut waits = Vec::new();
        for _ in 0..6 {
            schedule.failed(start, &transient());
            waits.push(schedule.deadline().unwrap() - start);
        }
        assert_eq!(waits, [5 * S, 10 * S, 20 * S, 40 * S, 40 * S, 40 * S]);
    }

    #[test]
    fn a_save_during_a_backoff_waits_for_it() {
        let (mut schedule, start) = schedule();
        schedule.failed(start, &transient());

        schedule.soon(start + S);

        assert_eq!(schedule.deadline(), Some(start + 5 * S));
    }

    #[test]
    fn a_success_ends_the_backoff() {
        let (mut schedule, start) = schedule();
        schedule.failed(start, &transient());
        schedule.failed(start, &transient());
        schedule.succeeded(start + 10 * S);

        schedule.saved(start + 11 * S);
        assert_eq!(schedule.deadline(), Some(start + 13 * S));
        // And the next failure starts from the base again.
        schedule.failed(start + 20 * S, &transient());
        assert_eq!(schedule.deadline(), Some(start + 25 * S));
    }

    #[test]
    fn a_rate_limit_is_waited_out_if_it_asks_for_longer_than_the_backoff() {
        let (mut schedule, start) = schedule();
        let limited = |secs| -> SyncError {
            RemoteError::RateLimited {
                retry_after: secs * S,
            }
            .into()
        };

        schedule.failed(start, &limited(30));
        assert_eq!(schedule.deadline(), Some(start + 30 * S));

        schedule.succeeded(start + 30 * S);
        schedule.failed(start + 30 * S, &limited(1));
        assert_eq!(schedule.deadline(), Some(start + 35 * S));
    }

    #[test]
    fn a_failure_that_retrying_cannot_fix_halts_until_a_pass_succeeds() {
        for error in [
            SyncError::Remote(RemoteError::Unauthorized),
            SyncError::KeyMismatch,
        ] {
            let (mut schedule, start) = schedule();
            schedule.failed(start, &error);

            assert_eq!(schedule.deadline(), None, "{error}");
            // Saves and timers do not wake it.
            schedule.saved(start + S);
            schedule.soon(start + 2 * S);
            assert_eq!(schedule.deadline(), None, "{error}");

            // The user's own retry succeeding does.
            schedule.succeeded(start + 3 * S);
            assert_eq!(schedule.deadline(), Some(start + 63 * S));
        }
    }

    #[test]
    fn an_unrecognised_failure_is_retried_slowly_not_abandoned() {
        let (mut schedule, start) = schedule();
        schedule.failed(start, &RemoteError::Other("odd".into()).into());
        assert_eq!(schedule.deadline(), Some(start + 5 * S));
    }

    /// `interval: None`: the startup pass is still due, and there is no
    /// periodic deadline afterwards — but a save still schedules a pass.
    #[test]
    fn no_interval_has_no_periodic_pass_but_a_save_still_schedules_one() {
        let no_interval = Timing {
            interval: None,
            ..timing()
        };
        let start = Instant::now();
        let mut schedule = Schedule::new(no_interval, start);
        assert_eq!(schedule.deadline(), Some(start), "the startup pass");

        schedule.succeeded(start);
        assert_eq!(schedule.deadline(), None, "no periodic deadline");

        schedule.saved(start + 5 * S);
        assert_eq!(schedule.deadline(), Some(start + 7 * S));
    }

    #[test]
    fn set_interval_moves_the_deadline() {
        let (mut schedule, start) = schedule();
        schedule.succeeded(start);
        assert_eq!(schedule.deadline(), Some(start + 60 * S));

        schedule.set_interval(Some(10 * S), start + 5 * S);
        assert_eq!(schedule.deadline(), Some(start + 15 * S));

        schedule.set_interval(None, start + 5 * S);
        assert_eq!(schedule.deadline(), None);
    }

    /// A `set_interval` landing exactly when the startup pass is already due
    /// must not cancel or delay it.
    #[test]
    fn set_interval_does_not_delay_a_pass_that_is_already_due() {
        let (mut schedule, start) = schedule();
        assert_eq!(schedule.deadline(), Some(start), "the startup pass is due");

        schedule.set_interval(None, start);

        assert_eq!(schedule.deadline(), Some(start));
    }

    /// With `interval: None`, a failed pass still schedules its own backoff
    /// retry (D8): only a *successful* pass leaves no periodic deadline.
    #[test]
    fn a_failed_pass_still_backs_off_with_no_interval() {
        let no_interval = Timing {
            interval: None,
            ..timing()
        };
        let start = Instant::now();
        let mut schedule = Schedule::new(no_interval, start);
        schedule.succeeded(start);
        assert_eq!(schedule.deadline(), None);

        schedule.failed(start, &transient());
        assert_eq!(schedule.deadline(), Some(start + 5 * S));
    }

    /// `set_interval` during a pending backoff retry must not cancel it
    /// (`None`) or bring it forward past the backoff: it only stores the new
    /// interval, which the next `succeeded` applies.
    #[test]
    fn set_interval_during_a_backoff_keeps_the_pending_retry() {
        let (mut schedule, start) = schedule();
        schedule.failed(start, &transient());
        assert_eq!(schedule.deadline(), Some(start + 5 * S));

        schedule.set_interval(None, start + S);
        assert_eq!(
            schedule.deadline(),
            Some(start + 5 * S),
            "the backoff retry must survive"
        );

        // The next success applies the new interval.
        schedule.succeeded(start + 5 * S);
        assert_eq!(schedule.deadline(), None);
    }
}
