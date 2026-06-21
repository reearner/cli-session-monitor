//! Discover pre-existing sessions from on-disk session files.
//!
//! Hooks/notify only fire once the user interacts, so a Claude/Codex session that
//! was already open when the app started stays invisible until the user types.
//! This scans the CLIs' session files directly and emits `Discovered` events so
//! those sessions show up (as idle) and can be jumped to right away.

use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use csm_core::{paths, Event, EventKind, Source};

use crate::codex_rollout::{read_head_cwd, thread_id_from_filename};

/// Cap on how many discovered sessions to surface (newest first).
const CAP: usize = 40;

/// Scan Codex rollouts + Claude transcripts modified within `recent_secs` and
/// return `Discovered` events (newest first, capped). Never errors — missing
/// dirs / unreadable files are skipped.
pub fn discover_sessions(recent_secs: u64) -> Vec<Event> {
    let now = now_ms();
    let cutoff = now - (recent_secs as i64) * 1000;
    let host = paths::host_name();
    let mut found: Vec<(i64, Event)> = Vec::new();

    collect_codex(&paths::codex_sessions_dir(), cutoff, &host, &mut found);
    collect_claude(&paths::claude_projects_dir(), cutoff, &host, &mut found);

    // Each CLI run is a separate session file, so one project dir accumulates
    // many. Collapse to one card per (source, host, cwd): keep the newest.
    let mut newest: HashMap<(Source, String, String), (i64, Event)> = HashMap::new();
    for (mtime, ev) in found {
        let key = (ev.source, ev.host.clone(), ev.cwd.clone());
        match newest.get(&key) {
            Some((m, _)) if *m >= mtime => {}
            _ => {
                newest.insert(key, (mtime, ev));
            }
        }
    }
    let mut found: Vec<(i64, Event)> = newest.into_values().collect();
    found.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime)); // newest first
    found.truncate(CAP);
    found.into_iter().map(|(_, e)| e).collect()
}

fn collect_codex(dir: &Path, cutoff: i64, host: &str, out: &mut Vec<(i64, Event)>) {
    for path in walk(dir, 5) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.starts_with("rollout-") && name.ends_with(".jsonl")) {
            continue;
        }
        let Some(mtime) = mtime_ms(&path) else {
            continue;
        };
        if mtime < cutoff {
            continue;
        }
        let Some(session_id) = thread_id_from_filename(&path) else {
            continue;
        };
        let cwd = read_head_cwd(&path).unwrap_or_default();
        out.push((
            mtime,
            Event::new(
                Source::Codex,
                session_id,
                cwd,
                host,
                EventKind::Discovered,
                mtime,
            ),
        ));
    }
}

fn collect_claude(dir: &Path, cutoff: i64, host: &str, out: &mut Vec<(i64, Event)>) {
    for path in walk(dir, 3) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".jsonl") {
            continue;
        }
        let Some(mtime) = mtime_ms(&path) else {
            continue;
        };
        if mtime < cutoff {
            continue;
        }
        let session_id = name.trim_end_matches(".jsonl").to_string();
        if session_id.is_empty() {
            continue;
        }
        let cwd = read_claude_cwd(&path).unwrap_or_default();
        out.push((
            mtime,
            Event::new(
                Source::ClaudeCode,
                session_id,
                cwd,
                host,
                EventKind::Discovered,
                mtime,
            ),
        ));
    }
}

/// The CURRENT `cwd` of a Claude session — the LAST one in the transcript, not
/// the first: a session's working directory can change mid-session (e.g. running
/// commands in a subdir), and the live hooks report the current cwd, so reading
/// the first would create a stale duplicate card. Reads only the file tail.
fn read_claude_cwd(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let tail = 256 * 1024u64;
    let start = len.saturating_sub(tail);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // last `"cwd":"..."` in the tail; unescape JSON backslashes.
    let needle = "\"cwd\":\"";
    let at = text.rfind(needle)? + needle.len();
    let rest = &text[at..];
    let end = rest.find('"')?;
    let cwd = rest[..end].replace("\\\\", "\\");
    (!cwd.is_empty()).then_some(cwd)
}

/// Recursively list files under `dir` up to `max_depth` (cheap, no deps).
fn walk(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_into(dir, max_depth, &mut files);
    files
}

fn walk_into(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                walk_into(&path, depth - 1, files);
            }
        } else {
            files.push(path);
        }
    }
}

fn mtime_ms(path: &Path) -> Option<i64> {
    let m = fs::metadata(path).ok()?.modified().ok()?;
    let d = m.duration_since(UNIX_EPOCH).ok()?;
    Some(d.as_millis() as i64)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
