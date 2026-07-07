//! The session state machine — the heart of the app.
//!
//! It turns a stream of normalized [`Event`]s into per-session status + timing,
//! emits a [`Effect::Completed`] when a turn finishes (for notifications), and
//! produces [`SessionView`] snapshots for the UI.
//!
//! Pure logic: no IO, no clock access. `tick` takes `now` as a parameter so the
//! whole module is deterministically testable without any CLI installed.

use std::collections::{HashMap, HashSet};

use csm_core::{Event, EventKind, SessionKey, Source};
use serde::Serialize;

/// Visible lifecycle state of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Between `run_start` and `run_end` — the timer is running.
    Running,
    /// Paused mid-turn awaiting the user (e.g. Codex approval prompt).
    Waiting,
    /// A turn just finished.
    Done,
    /// Finished a while ago (older than the idle threshold) — visually dimmed.
    Idle,
}

/// Snapshot of one session for the frontend. The live timer is computed on the
/// frontend from `run_started_at`, so the backend only pushes on change.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionView {
    pub key: SessionKey,
    pub source: Source,
    pub host: String,
    pub cwd: String,
    pub status: SessionStatus,
    /// Start of the current run (only meaningful while `Running`).
    pub run_started_at: Option<i64>,
    pub run_ended_at: Option<i64>,
    /// Duration of the last completed run; `None` if it couldn't be measured.
    pub last_duration_ms: Option<i64>,
    /// `false` when timing is an estimate / unavailable (e.g. a `run_end` with no
    /// preceding `run_start`, as can happen with Codex). UI should annotate this.
    pub timing_reliable: bool,
}

/// Something the caller should act on as a result of [`StateMachine::apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// A turn finished for this session — trigger a completion notification.
    Completed(SessionKey),
    /// The session is now waiting for the user (approval/input) — alert them.
    AwaitingInput(SessionKey),
}

#[derive(Debug, Clone)]
struct SessionState {
    source: Source,
    host: String,
    cwd: String,
    status: SessionStatus,
    run_started_at: Option<i64>,
    run_ended_at: Option<i64>,
    last_duration_ms: Option<i64>,
    timing_reliable: bool,
    last_event_at: i64,
    /// Event `ts` when this session was first seen. Stable — never updated — so
    /// snapshot ordering within a status group doesn't reshuffle as new events
    /// arrive; a card keeps its slot until its STATUS changes.
    first_seen: i64,
    /// True for placeholder cards synthesized from on-disk session files (not a
    /// live event). Used to de-dupe per directory and to let real events replace.
    discovered: bool,
    /// Local (monotonic-ish wall) time we first observed this session as `Done`,
    /// used for the idle-decay timer. Stamped by `tick`, NOT from the event's
    /// `ts` — a remote event's `ts` is on the remote clock, so using it would
    /// make a finished remote session jump straight to idle under clock skew.
    done_since: Option<i64>,
}

impl SessionState {
    fn seed(ev: &Event) -> Self {
        Self {
            source: ev.source,
            host: ev.host.clone(),
            cwd: ev.cwd.clone(),
            // Neutral until the first event sets the real status; must NOT be
            // Running, or a fresh RunStart would be mistaken for a continuation.
            status: SessionStatus::Idle,
            run_started_at: None,
            run_ended_at: None,
            last_duration_ms: None,
            timing_reliable: true,
            last_event_at: ev.ts,
            first_seen: ev.ts,
            discovered: false,
            done_since: None,
        }
    }
}

/// Tracks all known sessions keyed by [`SessionKey`] (source + host + id), which
/// keeps local and remote sessions distinct.
pub struct StateMachine {
    sessions: HashMap<SessionKey, SessionState>,
    idle_threshold_ms: i64,
    /// Sessions the user closed from the panel. Hidden from snapshots until the
    /// session shows fresh activity (any `apply`ed event un-hides it). The session
    /// itself is kept so a discovered placeholder for the same dir stays suppressed.
    dismissed: HashSet<SessionKey>,
}

impl StateMachine {
    pub fn new(idle_threshold_secs: u32) -> Self {
        Self {
            sessions: HashMap::new(),
            idle_threshold_ms: i64::from(idle_threshold_secs) * 1000,
            dismissed: HashSet::new(),
        }
    }

    /// Hide a session card (user closed it). Returns whether the visible set
    /// changed. The session reappears if it later produces a new event.
    pub fn dismiss(&mut self, key: &SessionKey) -> bool {
        self.sessions.contains_key(key) && self.dismissed.insert(key.clone())
    }

    /// Update the idle threshold at runtime (when the user changes config).
    pub fn set_idle_threshold_secs(&mut self, secs: u32) {
        self.idle_threshold_ms = i64::from(secs) * 1000;
    }

    /// Apply one event, returning any effects (currently: completion signals).
    pub fn apply(&mut self, ev: Event) -> Vec<Effect> {
        // A discovered placeholder is keyed by the session's REAL id (same as the
        // live key), so a live event for that session updates the very same card —
        // we just clear its `discovered` flag (below) so it's no longer a
        // placeholder. No dir-based de-dup needed: distinct sessions in one dir are
        // distinct cards, and the same session folds onto itself by id.
        let key = SessionKey::of(&ev);
        // Fresh activity un-hides a session the user had closed.
        self.dismissed.remove(&key);
        match ev.event {
            EventKind::RunStart => {
                let s = self
                    .sessions
                    .entry(key)
                    .or_insert_with(|| SessionState::seed(&ev));
                s.discovered = false; // a live event: this is a real session now
                if !ev.cwd.is_empty() {
                    s.cwd = ev.cwd;
                }
                // Coming from Running/Waiting means a continuation or a resume
                // after approval — keep the existing start so the timer doesn't
                // reset mid-turn. Otherwise it's a fresh turn.
                let continuing =
                    matches!(s.status, SessionStatus::Running | SessionStatus::Waiting);
                if !continuing {
                    s.run_started_at = Some(ev.ts);
                    s.timing_reliable = true;
                }
                s.status = SessionStatus::Running;
                s.run_ended_at = None;
                s.last_event_at = ev.ts;
                s.done_since = None;
                Vec::new()
            }
            EventKind::WaitingInput => {
                let s = self
                    .sessions
                    .entry(key.clone())
                    .or_insert_with(|| SessionState::seed(&ev));
                s.discovered = false; // a live event: this is a real session now
                if !ev.cwd.is_empty() {
                    s.cwd = ev.cwd;
                }
                // Only notify on the TRANSITION into waiting — Claude's Notification
                // hook can fire several times during one pause, and re-emitting would
                // pop a duplicate toast each time.
                let entering = s.status != SessionStatus::Waiting;
                s.status = SessionStatus::Waiting; // keep run_started_at (timer continues)
                s.last_event_at = ev.ts;
                s.done_since = None;
                if entering {
                    vec![Effect::AwaitingInput(key)]
                } else {
                    Vec::new()
                }
            }
            EventKind::RunEnd => {
                let s = self
                    .sessions
                    .entry(key.clone())
                    .or_insert_with(|| SessionState::seed(&ev));
                s.discovered = false; // a live event: this is a real session now
                if !ev.cwd.is_empty() {
                    s.cwd = ev.cwd;
                }
                // Reliable only if we have a fresh start for *this* run; consume
                // it so a subsequent run_end with no run_start degrades correctly.
                let (duration, reliable) = match s.run_started_at.take() {
                    Some(start) if ev.ts >= start => (Some(ev.ts - start), true),
                    _ => (None, false),
                };
                s.status = SessionStatus::Done;
                s.run_ended_at = Some(ev.ts);
                s.last_duration_ms = duration;
                s.timing_reliable = reliable;
                s.last_event_at = ev.ts;
                s.done_since = None; // tick stamps the local decay start
                vec![Effect::Completed(key)]
            }
            EventKind::SessionEnd => {
                self.sessions.remove(&key);
                Vec::new()
            }
            // Discovered placeholders are managed via `reconcile_discovered`
            // (which also classifies idle vs waiting by window existence), not the
            // event path.
            EventKind::Discovered => Vec::new(),
        }
    }

    /// Advance time: a `Done` session becomes `Idle` once it has *been observed*
    /// done for the idle threshold. `now` is local wall time; the decay is timed
    /// from when we first saw it done (stamped here), not the event's `ts`, so a
    /// finished remote session with a skewed clock doesn't jump straight to idle.
    pub fn tick(&mut self, now: i64) -> bool {
        let threshold = self.idle_threshold_ms;
        let mut changed = false;
        for s in self.sessions.values_mut() {
            if s.status == SessionStatus::Done {
                match s.done_since {
                    None => s.done_since = Some(now), // first observed done; not visible
                    Some(t) if now - t >= threshold => {
                        s.status = SessionStatus::Idle;
                        s.done_since = None;
                        changed = true;
                    }
                    _ => {}
                }
            }
        }
        changed
    }

    /// Reconcile discovered (on-disk) sessions against the latest scan. `items` is
    /// `(event, active)` — `active` means the CLI was used recently and its editor
    /// window is open, so the placeholder shows as `Waiting`; otherwise `Idle`.
    /// (A genuinely-live CLI is shown via the event stream as running/waiting.)
    /// De-dupes to one placeholder per directory, drops placeholders whose dir
    /// left the scan, and yields to real sessions. Returns whether anything changed.
    pub fn reconcile_discovered(&mut self, items: Vec<(Event, bool)>) -> bool {
        let mut changed = false;
        // Placeholders are keyed by the session's REAL id (source+host+id), so each
        // on-disk session gets its own resumable card — several in one directory
        // stay distinct instead of collapsing to one.
        let present: HashSet<SessionKey> = items.iter().map(|(e, _)| SessionKey::of(e)).collect();
        // Ids of live (non-placeholder) sessions: the same session showing live
        // supersedes its own placeholder, so we neither keep nor re-create it.
        let live_ids: HashSet<SessionKey> = self
            .sessions
            .iter()
            .filter(|(_, s)| !s.discovered)
            .map(|(k, _)| k.clone())
            .collect();
        let before = self.sessions.len();
        self.sessions
            .retain(|k, s| !s.discovered || (present.contains(k) && !live_ids.contains(k)));
        changed |= self.sessions.len() != before;
        // Drop dismiss marks whose session is gone, so the set can't grow without
        // bound or wrongly suppress a future session that reuses the key.
        self.dismissed.retain(|k| self.sessions.contains_key(k));

        for (ev, active) in items {
            if ev.cwd.is_empty() {
                continue;
            }
            let key = SessionKey::of(&ev);
            // The same session is already live — its real card supersedes this one.
            if live_ids.contains(&key) {
                continue;
            }
            let want = if active {
                SessionStatus::Waiting
            } else {
                SessionStatus::Idle
            };
            match self.sessions.get_mut(&key) {
                Some(s) if s.discovered => {
                    if s.status != want {
                        s.status = want;
                        changed = true;
                    }
                }
                Some(_) => {} // a live session at this key — leave it untouched
                None => {
                    let mut st = SessionState::seed(&ev);
                    st.discovered = true;
                    st.status = want;
                    self.sessions.insert(key, st);
                    changed = true;
                }
            }
        }
        changed
    }

    /// Current sessions, ordered by status (`Waiting` > `Running` > `Done` >
    /// `Idle`), then STABLY within each status. The order is deliberately NOT
    /// recency-based: sorting by `last_event_at` made same-status cards jump
    /// around every time a session emitted an event (e.g. a Claude tool call), so
    /// reopening the panel showed a reshuffled list. Now a card keeps a fixed slot
    /// (by first-seen, then id) and only moves when its STATUS changes.
    pub fn snapshot(&self) -> Vec<SessionView> {
        let mut entries: Vec<(&SessionKey, &SessionState)> = self
            .sessions
            .iter()
            .filter(|(k, _)| !self.dismissed.contains(k))
            .collect();
        entries.sort_by(|(ka, a), (kb, b)| {
            status_rank(a.status)
                .cmp(&status_rank(b.status))
                // real sessions before discovered placeholders of the same status
                .then(a.discovered.cmp(&b.discovered))
                // stable within a status: oldest-seen first (new sessions append),
                // id as a final deterministic tiebreak
                .then(a.first_seen.cmp(&b.first_seen))
                .then(ka.session_id.cmp(&kb.session_id))
        });
        entries
            .into_iter()
            .map(|(key, s)| SessionView {
                key: key.clone(),
                source: s.source,
                host: s.host.clone(),
                cwd: s.cwd.clone(),
                status: s.status,
                run_started_at: s.run_started_at,
                run_ended_at: s.run_ended_at,
                last_duration_ms: s.last_duration_ms,
                timing_reliable: s.timing_reliable,
            })
            .collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

fn status_rank(s: SessionStatus) -> u8 {
    match s {
        SessionStatus::Waiting => 0, // needs your attention first
        SessionStatus::Running => 1,
        SessionStatus::Done => 2,
        SessionStatus::Idle => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(source: Source, id: &str, host: &str, kind: EventKind, ts: i64) -> Event {
        Event::new(source, id, "/proj", host, kind, ts)
    }

    fn cc(id: &str, kind: EventKind, ts: i64) -> Event {
        ev(Source::ClaudeCode, id, "host", kind, ts)
    }

    #[test]
    fn run_start_then_end_computes_duration() {
        let mut sm = StateMachine::new(120);
        assert!(sm.apply(cc("s", EventKind::RunStart, 1000)).is_empty());
        let eff = sm.apply(cc("s", EventKind::RunEnd, 4000));
        assert_eq!(eff.len(), 1);

        let v = &sm.snapshot()[0];
        assert_eq!(v.status, SessionStatus::Done);
        assert_eq!(v.last_duration_ms, Some(3000));
        assert!(v.timing_reliable);
    }

    #[test]
    fn run_end_without_start_is_done_but_unreliable() {
        let mut sm = StateMachine::new(120);
        let eff = sm.apply(ev(Source::Codex, "c", "host", EventKind::RunEnd, 5000));
        assert_eq!(eff.len(), 1, "completion still fires");

        let v = &sm.snapshot()[0];
        assert_eq!(v.status, SessionStatus::Done);
        assert_eq!(v.last_duration_ms, None);
        assert!(!v.timing_reliable);
    }

    #[test]
    fn repeated_run_end_without_new_start_stays_unreliable() {
        // Mirrors Codex: only run_end events arrive, one per turn.
        let mut sm = StateMachine::new(120);
        sm.apply(ev(Source::Codex, "c", "host", EventKind::RunEnd, 1000));
        sm.apply(ev(Source::Codex, "c", "host", EventKind::RunEnd, 2000));
        let v = &sm.snapshot()[0];
        assert!(!v.timing_reliable);
        assert_eq!(v.last_duration_ms, None);
        assert_eq!(sm.len(), 1, "same session, not two cards");
    }

    #[test]
    fn dismiss_hides_card_until_fresh_activity() {
        let mut sm = StateMachine::new(120);
        sm.apply(cc("s", EventKind::RunEnd, 1000));
        assert_eq!(sm.snapshot().len(), 1);

        let key = SessionKey {
            source: Source::ClaudeCode,
            host: "host".into(),
            session_id: "s".into(),
        };
        assert!(
            sm.dismiss(&key),
            "dismissing a present session changes the view"
        );
        assert!(sm.snapshot().is_empty(), "dismissed card is hidden");
        assert!(!sm.dismiss(&key), "dismissing again is a no-op");

        // A new event for the session un-hides it.
        sm.apply(cc("s", EventKind::RunStart, 2000));
        let v = sm.snapshot();
        assert_eq!(v.len(), 1, "fresh activity brings the card back");
        assert_eq!(v[0].status, SessionStatus::Running);
    }

    #[test]
    fn dismiss_unknown_session_is_noop() {
        let mut sm = StateMachine::new(120);
        let key = SessionKey {
            source: Source::Codex,
            host: "host".into(),
            session_id: "nope".into(),
        };
        assert!(!sm.dismiss(&key), "nothing to hide -> no change");
    }

    #[test]
    fn placeholder_folds_onto_live_session_by_id_distinct_sessions_stay() {
        let mut sm = StateMachine::new(120);
        // A live session "real" that cd'd into a subdir (cwd comes from the hooks).
        sm.apply(Event::new(
            Source::ClaudeCode,
            "real",
            "D:\\proj\\sub",
            "host",
            EventKind::RunStart,
            1000,
        ));
        // Discovery surfaces: the SAME session (id "real") seen at its parent dir
        // (should fold by id), a DIFFERENT session ("p") in the same dir (should
        // stay — distinct sessions are distinct cards), and an unrelated one ("o").
        let same = Event::new(
            Source::ClaudeCode,
            "real",
            "D:\\proj",
            "host",
            EventKind::Discovered,
            2000,
        );
        let sibling = Event::new(
            Source::ClaudeCode,
            "p",
            "D:\\proj\\sub",
            "host",
            EventKind::Discovered,
            2500,
        );
        let other = Event::new(
            Source::ClaudeCode,
            "o",
            "D:\\other",
            "host",
            EventKind::Discovered,
            3000,
        );
        sm.reconcile_discovered(vec![(same, true), (sibling, true), (other, true)]);

        let snap = sm.snapshot();
        assert_eq!(snap.len(), 3, "live + sibling + unrelated (no dup for 'real')");
        let real: Vec<_> = snap.iter().filter(|v| v.key.session_id == "real").collect();
        assert_eq!(real.len(), 1, "the 'real' placeholder folded onto the live one");
        assert_eq!(real[0].status, SessionStatus::Running, "kept the live card");
        assert!(snap.iter().any(|v| v.key.session_id == "p"), "sibling session kept");
        assert!(snap.iter().any(|v| v.key.session_id == "o"), "unrelated kept");
    }

    #[test]
    fn lingering_placeholder_folds_when_its_session_goes_live() {
        let mut sm = StateMachine::new(120);
        let disc = || {
            Event::new(
                Source::ClaudeCode,
                "s",
                "D:\\proj",
                "host",
                EventKind::Discovered,
                1000,
            )
        };
        // 1) discovery creates a placeholder for session "s" (no real session yet)
        sm.reconcile_discovered(vec![(disc(), true)]);
        assert_eq!(sm.snapshot().len(), 1);
        // 2) that same session goes live in a subdir (its cwd cd'd in)
        sm.apply(Event::new(
            Source::ClaudeCode,
            "s",
            "D:\\proj\\sub",
            "host",
            EventKind::RunStart,
            2000,
        ));
        // 3) the next discovery cycle still lists "s", but it's live now — the
        //    placeholder must fold onto the live card (by id), not sit beside it.
        sm.reconcile_discovered(vec![(disc(), true)]);
        let snap = sm.snapshot();
        assert_eq!(snap.len(), 1, "placeholder folded onto the live session");
        assert_eq!(snap[0].status, SessionStatus::Running);
        assert_eq!(snap[0].cwd, "D:\\proj\\sub");
    }

    #[test]
    fn multiple_sessions_in_one_dir_each_get_a_card() {
        let mut sm = StateMachine::new(120);
        let mk = |id: &str, ts: i64| {
            Event::new(
                Source::ClaudeCode,
                id,
                "D:\\proj",
                "host",
                EventKind::Discovered,
                ts,
            )
        };
        sm.reconcile_discovered(vec![
            (mk("a", 1000), false),
            (mk("b", 2000), false),
            (mk("c", 3000), false),
        ]);
        let snap = sm.snapshot();
        assert_eq!(snap.len(), 3, "three sessions in one dir → three grey cards");
        for id in ["a", "b", "c"] {
            assert!(
                snap.iter().any(|v| v.key.session_id == id),
                "session {id} has its own resumable card"
            );
        }
    }

    #[test]
    fn restart_after_done_resets_to_running() {
        let mut sm = StateMachine::new(120);
        sm.apply(cc("s", EventKind::RunStart, 1000));
        sm.apply(cc("s", EventKind::RunEnd, 2000));
        sm.apply(cc("s", EventKind::RunStart, 5000));
        let v = &sm.snapshot()[0];
        assert_eq!(v.status, SessionStatus::Running);
        assert_eq!(v.run_started_at, Some(5000));
        assert_eq!(v.run_ended_at, None);
    }

    #[test]
    fn session_end_removes_session() {
        let mut sm = StateMachine::new(120);
        sm.apply(cc("s", EventKind::RunStart, 1000));
        assert_eq!(sm.len(), 1);
        sm.apply(cc("s", EventKind::SessionEnd, 2000));
        assert!(sm.is_empty());
    }

    #[test]
    fn codex_approval_flow_running_waiting_resume_done() {
        let mut sm = StateMachine::new(120);
        let key = SessionKey {
            source: Source::Codex,
            host: "host".into(),
            session_id: "t".into(),
        };
        // task_started -> running
        sm.apply(ev(Source::Codex, "t", "host", EventKind::RunStart, 1000));
        assert_eq!(sm.snapshot()[0].status, SessionStatus::Running);

        // approval request -> waiting (timer kept), emits AwaitingInput
        let eff = sm.apply(ev(
            Source::Codex,
            "t",
            "host",
            EventKind::WaitingInput,
            2000,
        ));
        assert_eq!(eff, vec![Effect::AwaitingInput(key)]);
        let v = &sm.snapshot()[0];
        assert_eq!(v.status, SessionStatus::Waiting);
        assert_eq!(v.run_started_at, Some(1000), "timer not reset on waiting");

        // resume (e.g. exec_command_begin -> RunStart) keeps original start
        sm.apply(ev(Source::Codex, "t", "host", EventKind::RunStart, 3000));
        let v = &sm.snapshot()[0];
        assert_eq!(v.status, SessionStatus::Running);
        assert_eq!(v.run_started_at, Some(1000), "resume keeps original start");

        // task_complete -> done with real duration
        sm.apply(ev(Source::Codex, "t", "host", EventKind::RunEnd, 9000));
        let v = &sm.snapshot()[0];
        assert_eq!(v.status, SessionStatus::Done);
        assert_eq!(v.last_duration_ms, Some(8000));
        assert!(v.timing_reliable);
    }

    #[test]
    fn tick_transitions_done_to_idle_after_threshold() {
        let mut sm = StateMachine::new(10); // 10s threshold
        sm.apply(cc("s", EventKind::RunStart, 0));
        sm.apply(cc("s", EventKind::RunEnd, 1000)); // ended at t=1s

        // Decay is timed from when tick first observes "done" (local clock), not
        // the event ts — so the first tick just stamps and the session stays Done.
        assert!(!sm.tick(5_000)); // first observe -> stamp done_since, still Done
        assert_eq!(sm.snapshot()[0].status, SessionStatus::Done);

        assert!(!sm.tick(14_000)); // 9s observed: still Done
        assert!(sm.tick(15_000)); // 10s observed: now Idle
        assert_eq!(sm.snapshot()[0].status, SessionStatus::Idle);

        assert!(!sm.tick(20_000)); // idempotent: no further change
    }

    #[test]
    fn running_session_is_not_made_idle() {
        let mut sm = StateMachine::new(1);
        sm.apply(cc("s", EventKind::RunStart, 0));
        assert!(!sm.tick(1_000_000));
        assert_eq!(sm.snapshot()[0].status, SessionStatus::Running);
    }

    #[test]
    fn multiple_sessions_and_hosts_are_isolated() {
        let mut sm = StateMachine::new(120);
        sm.apply(cc("s", EventKind::RunStart, 1000));
        sm.apply(ev(
            Source::ClaudeCode,
            "s",
            "other-host",
            EventKind::RunStart,
            1000,
        ));
        sm.apply(ev(Source::Codex, "s", "host", EventKind::RunEnd, 1000));
        assert_eq!(sm.len(), 3, "same id on different host/source => distinct");
    }

    #[test]
    fn snapshot_order_is_stable_within_a_status() {
        let mut sm = StateMachine::new(120);
        // Two running sessions; "a" seen before "b".
        sm.apply(cc("a", EventKind::RunStart, 10));
        sm.apply(cc("b", EventKind::RunStart, 20));
        let before: Vec<String> =
            sm.snapshot().iter().map(|v| v.key.session_id.clone()).collect();
        // "a" emits a newer event — recency ordering would have jumped it to the
        // front; first-seen ordering must keep the slots put.
        sm.apply(cc("a", EventKind::RunStart, 1000));
        let after: Vec<String> =
            sm.snapshot().iter().map(|v| v.key.session_id.clone()).collect();
        assert_eq!(before, after, "same-status cards keep their slot on new events");
        assert_eq!(after, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn snapshot_orders_running_before_done_before_idle() {
        let mut sm = StateMachine::new(10);
        sm.apply(cc("done", EventKind::RunStart, 0));
        sm.apply(cc("done", EventKind::RunEnd, 100));
        sm.apply(cc("idle", EventKind::RunStart, 0));
        sm.apply(cc("idle", EventKind::RunEnd, 100));
        sm.tick(1_000_000); // observe both done
        sm.tick(1_010_001); // >=10s later: pushes both done->idle
        sm.apply(cc("run", EventKind::RunStart, 2_000_000));
        // "done2" finished recently, stays Done
        sm.apply(cc("done2", EventKind::RunStart, 2_000_000));
        sm.apply(cc("done2", EventKind::RunEnd, 2_000_100));

        let snap = sm.snapshot();
        assert_eq!(snap[0].status, SessionStatus::Running);
        // last entries should be Idle
        assert_eq!(snap.last().unwrap().status, SessionStatus::Idle);
    }

    #[test]
    fn each_run_end_emits_one_completed_effect() {
        let mut sm = StateMachine::new(120);
        let e1 = sm.apply(cc("a", EventKind::RunEnd, 1));
        let e2 = sm.apply(cc("b", EventKind::RunEnd, 2));
        assert_eq!(
            e1,
            vec![Effect::Completed(SessionKey {
                source: Source::ClaudeCode,
                host: "host".into(),
                session_id: "a".into()
            })]
        );
        assert_eq!(e2.len(), 1);
    }
}
