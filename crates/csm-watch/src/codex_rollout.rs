//! Codex-specific event source: tails Codex's per-session **rollout** JSONL files
//! under `~/.codex/sessions/**/rollout-*.jsonl`.
//!
//! Maps Codex's turn lifecycle to our events:
//!   - `task_started`                  -> RunStart (running, live timer)
//!   - `task_complete` / `turn_aborted`-> RunEnd   (done, real timing)
//!
//! We notify on RunEnd, i.e. exactly when Codex **hands the floor back to the
//! user** (the turn ended — whether it finished the job or stopped to ask you
//! something). We intentionally do NOT try to flag "waiting" mid-turn: the only
//! observable signal (a `function_call` without output yet) can't tell an
//! approval pause apart from a long-running command, so it produced premature
//! "still working" alerts. Turn-end is the reliable hand-back signal.
//!
//! Only metadata is read (event type, ids, timestamps) — never command output
//! or conversation content. The user runs Codex normally; we just watch.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime};

use csm_core::{Event, EventKind, Source};
use notify::{RecursiveMode, Watcher};

use crate::Source as SourceTrait;

/// Poll fallback interval — Windows file watching can drop append events, so we
/// also re-scan recent rollouts on a timer.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Only poll files touched within this window (bounds the work).
const RECENT_WINDOW_SECS: u64 = 1800;

pub struct CodexRolloutSource {
    sessions_dir: PathBuf,
}

/// Per-rollout-file (== per-session) tracking.
#[derive(Default)]
struct FileState {
    offset: u64,
    cwd: String,
    session_id: String,
}

#[derive(Default)]
struct WatchState {
    files: HashMap<PathBuf, FileState>,
}

impl CodexRolloutSource {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            sessions_dir: dir.as_ref().to_path_buf(),
        }
    }

    /// `~/.codex/sessions`
    pub fn default_dir() -> PathBuf {
        csm_core::paths::codex_sessions_dir()
    }
}

impl SourceTrait for CodexRolloutSource {
    fn run(self, tx: Sender<Event>) {
        let dir = self.sessions_dir;
        if !dir.exists() {
            return; // no Codex here
        }

        let mut state = WatchState::default();
        record_existing(&dir, &mut state); // don't replay history

        let (raw_tx, raw_rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("codex-rollout: watcher init failed: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive) {
            eprintln!("codex-rollout: watch failed: {e}");
            return;
        }

        loop {
            match raw_rx.recv_timeout(POLL_INTERVAL) {
                Ok(Ok(event)) if is_relevant(&event.kind) => {
                    for path in event.paths {
                        if is_rollout(&path) {
                            process_appended(&path, &mut state, &tx);
                        }
                    }
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => eprintln!("codex-rollout: event error: {e}"),
                // Belt-and-suspenders: re-scan recent rollouts in case the OS
                // file watcher dropped an append (happens on Windows).
                Err(RecvTimeoutError::Timeout) => poll_recent(&dir, &mut state, &tx),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

/// Re-read appended content from rollouts modified within the recent window —
/// catches events the file watcher missed, and picks up new sessions.
fn poll_recent(dir: &Path, state: &mut WatchState, tx: &Sender<Event>) {
    let now = SystemTime::now();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = match std::fs::read_dir(&d) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !is_rollout(&path) {
                continue;
            }
            let recent = meta
                .modified()
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .map(|d| d.as_secs() <= RECENT_WINDOW_SECS)
                .unwrap_or(false);
            if !recent {
                continue;
            }
            if !state.files.contains_key(&path) {
                // a session we haven't seen (watcher missed its Create) — read
                // it from the start so we don't lose its events.
                let session_id = thread_id_from_filename(&path).unwrap_or_default();
                let cwd = read_head_cwd(&path).unwrap_or_default();
                state.files.insert(
                    path.clone(),
                    FileState {
                        offset: 0,
                        session_id,
                        cwd,
                    },
                );
            }
            process_appended(&path, state, tx);
        }
    }
}

fn is_relevant(kind: &notify::EventKind) -> bool {
    use notify::EventKind::{Create, Modify};
    matches!(kind, Create(_) | Modify(_))
}

fn is_rollout(path: &Path) -> bool {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.starts_with("rollout-") && n.ends_with(".jsonl"),
        None => false,
    }
}

pub(crate) fn thread_id_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 5 {
        return None;
    }
    let uuid = parts[parts.len() - 5..].join("-");
    if uuid.rsplit('-').next().map(|s| s.len()) == Some(12) {
        Some(uuid)
    } else {
        None
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Read the first few lines of a rollout for a `payload.cwd` (session_meta /
/// turn_context), so sessions already running at startup still show a directory.
pub(crate) fn read_head_cwd(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    for _ in 0..6 {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim_end()) {
            if let Some(cwd) = v
                .get("payload")
                .and_then(|p| p.get("cwd"))
                .and_then(|c| c.as_str())
            {
                if !cwd.is_empty() {
                    return Some(cwd.to_string());
                }
            }
        }
    }
    None
}

fn record_existing(dir: &Path, state: &mut WatchState) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = match std::fs::read_dir(&d) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_rollout(&path) {
                let len = path.metadata().map(|m| m.len()).unwrap_or(0);
                let session_id = thread_id_from_filename(&path).unwrap_or_default();
                // Read the head for cwd (in session_meta) now, since we skip the
                // file's existing body — otherwise sessions already running at
                // startup would show no directory.
                let cwd = read_head_cwd(&path).unwrap_or_default();
                state.files.insert(
                    path,
                    FileState {
                        offset: len,
                        session_id,
                        cwd,
                    },
                );
            }
        }
    }
}

fn process_appended(path: &Path, state: &mut WatchState, tx: &Sender<Event>) {
    let fs = state
        .files
        .entry(path.to_path_buf())
        .or_insert_with(|| FileState {
            session_id: thread_id_from_filename(path).unwrap_or_default(),
            ..Default::default()
        });

    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len < fs.offset {
        fs.offset = 0;
    }
    if file.seek(SeekFrom::Start(fs.offset)).is_err() {
        return;
    }

    let mut reader = BufReader::new(file);
    let mut consumed = fs.offset;
    loop {
        let mut line = String::new();
        let n = match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if !line.ends_with('\n') {
            break; // partial; wait for more
        }
        consumed += n as u64;
        handle_line(fs, line.trim_end(), tx);
    }
    fs.offset = consumed;
}

/// Apply one rollout line: cache cwd, or emit a run event on turn start/end.
fn handle_line(fs: &mut FileState, line: &str, tx: &Sender<Event>) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return,
    };
    let outer = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let ptype = v
        .get("payload")
        .and_then(|p| p.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    let kind = match (outer, ptype) {
        // cwd appears in session_meta (file start) and every turn_context.
        ("session_meta", _) | ("turn_context", _) => {
            if let Some(cwd) = v
                .get("payload")
                .and_then(|p| p.get("cwd"))
                .and_then(|c| c.as_str())
            {
                if !cwd.is_empty() {
                    fs.cwd = cwd.to_string();
                }
            }
            return;
        }
        ("event_msg", "task_started") => EventKind::RunStart,
        ("event_msg", "task_complete") | ("event_msg", "turn_aborted") => EventKind::RunEnd,
        _ => return,
    };

    let _ = tx.send(Event::new(
        Source::Codex,
        fs.session_id.clone(),
        fs.cwd.clone(),
        csm_core::host_name(),
        kind,
        now_ms(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn rollout_path() -> PathBuf {
        PathBuf::from("rollout-2026-06-14T01-39-51-019ec211-90f2-7b40-a06b-e98a4a940386.jsonl")
    }

    fn new_fs() -> FileState {
        FileState {
            session_id: thread_id_from_filename(&rollout_path()).unwrap(),
            ..Default::default()
        }
    }

    #[test]
    fn extracts_thread_id_from_filename() {
        assert_eq!(
            thread_id_from_filename(&rollout_path()).as_deref(),
            Some("019ec211-90f2-7b40-a06b-e98a4a940386")
        );
    }

    #[test]
    fn is_rollout_matches_only_rollout_jsonl() {
        assert!(is_rollout(&rollout_path()));
        assert!(!is_rollout(Path::new("history.jsonl")));
    }

    #[test]
    fn task_started_and_complete_map_to_run_events_with_cwd() {
        let mut fs = new_fs();
        let (tx, rx) = channel();
        handle_line(
            &mut fs,
            r#"{"type":"session_meta","payload":{"cwd":"D:\\proj"}}"#,
            &tx,
        );
        handle_line(
            &mut fs,
            r#"{"type":"event_msg","payload":{"type":"task_started"}}"#,
            &tx,
        );
        let e = rx.recv().unwrap();
        assert_eq!(e.event, EventKind::RunStart);
        assert_eq!(e.cwd, "D:\\proj");
        assert_eq!(e.session_id, "019ec211-90f2-7b40-a06b-e98a4a940386");

        handle_line(
            &mut fs,
            r#"{"type":"event_msg","payload":{"type":"task_complete"}}"#,
            &tx,
        );
        assert_eq!(rx.recv().unwrap().event, EventKind::RunEnd);
        handle_line(
            &mut fs,
            r#"{"type":"event_msg","payload":{"type":"turn_aborted"}}"#,
            &tx,
        );
        assert_eq!(rx.recv().unwrap().event, EventKind::RunEnd);
    }

    #[test]
    fn running_command_does_not_emit_anything_premature() {
        // a function_call (command running, still working) must NOT notify
        let mut fs = new_fs();
        let (tx, rx) = channel();
        handle_line(
            &mut fs,
            r#"{"type":"response_item","payload":{"type":"function_call","call_id":"c1"}}"#,
            &tx,
        );
        assert!(rx.try_recv().is_err(), "no event while a command runs");
    }

    #[test]
    fn ignores_content_lines() {
        let mut fs = new_fs();
        let (tx, rx) = channel();
        handle_line(
            &mut fs,
            r#"{"type":"response_item","payload":{"type":"agent_message","text":"secret"}}"#,
            &tx,
        );
        assert!(rx.try_recv().is_err());
    }
}
