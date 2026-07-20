//! csm-agent — runs on a (remote) machine where Codex/Claude execute, watches
//! their session files, and publishes normalized events to an ntfy relay topic
//! that the desktop app subscribes to. No SSH required; only metadata is sent.
//! Redundant same-kind bursts (e.g. a RunStart per Claude tool call) are coalesced
//! so the relay isn't flooded, and a rate-limited (429) relay makes the agent back
//! off exponentially rather than hammer it.
//!
//! Config via env:
//!   CSM_RELAY_TOPIC  (required)  the ntfy topic to publish to
//!   CSM_RELAY_URL    (optional)  default https://ntfy.sh
//!   CSM_RELAY_TOKEN  (optional)  ntfy access token
//!   CSM_WATCH_DIRS   (optional)  colon-separated whitelist of project dirs; when
//!                                set, ONLY sessions whose cwd is inside one of them
//!                                are relayed (others never leave this host). The
//!                                generated remote-agent.sh sets it via --include-dir.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use csm_core::{Event, EventKind, SessionKey};
use csm_watch::ntfy::PublishError;
use csm_watch::{ntfy, CodexRolloutSource, FsWatchSource, Source};

/// ntfy refills a visitor's request bucket at roughly one request per 5s, so a
/// rate-limited publish waits at least that long before trying again.
const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5);
/// Other failures (e.g. a network blip) start with a shorter pause.
const ERROR_BACKOFF: Duration = Duration::from_secs(1);
/// Upper bound, so a long outage doesn't stall the agent forever.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Next wait after a failed publish: start at the kind-appropriate base, then
/// double on each further failure, capped. Hammering a relay that answered 429 only
/// keeps its bucket empty (and can get the IP blocked), so we always wait.
fn next_backoff(current: Option<Duration>, rate_limited: bool) -> Duration {
    let base = if rate_limited {
        RATE_LIMIT_BACKOFF
    } else {
        ERROR_BACKOFF
    };
    match current {
        None => base,
        Some(d) => (d * 2).min(MAX_BACKOFF).max(base),
    }
}

/// Parse `CSM_WATCH_DIRS` (colon-separated, PATH-style) into a whitelist, dropping
/// blank entries. An empty result means "no filter — relay every session".
fn parse_watch_dirs(raw: &str) -> Vec<String> {
    raw.split(':')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Whether an event may be relayed given the whitelist. Empty whitelist => always
/// (no filtering). With a whitelist, a session is relayed only if its cwd is inside
/// a watched dir; an empty cwd is rejected (we can't confirm it's in scope).
fn cwd_allowed(cwd: &str, watch: &[String]) -> bool {
    watch.is_empty()
        || (!cwd.is_empty() && watch.iter().any(|w| csm_core::pathmatch::is_under(cwd, w)))
}

/// Whether an event just repeats the last kind already published for its session,
/// carrying no new visible state. Claude fires PreToolUse+PostToolUse (both map to
/// `RunStart`) on EVERY tool call, but the card is already "running" — relaying each
/// one only burns ntfy quota (hitting 429s). Coalescing these away cuts relay
/// traffic by an order of magnitude. A real state CHANGE (RunStart after Waiting,
/// RunEnd, …) differs from the last kind, so it is never suppressed.
fn is_redundant(last_for_session: Option<EventKind>, ev: EventKind) -> bool {
    last_for_session == Some(ev)
}

fn main() {
    // Report and exit, so a version can be checked without starting the relay.
    // NOTE: versions before this one ignored argv completely and would just START
    // RELAYING, so the launcher must never probe an unknown binary with this.
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("csm-agent {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let topic = match std::env::var("CSM_RELAY_TOPIC") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            eprintln!("csm-agent: set CSM_RELAY_TOPIC (the ntfy topic to publish to)");
            std::process::exit(2);
        }
    };
    let base = std::env::var("CSM_RELAY_URL").unwrap_or_else(|_| "https://ntfy.sh".to_string());
    let token = std::env::var("CSM_RELAY_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    let watch_dirs = std::env::var("CSM_WATCH_DIRS")
        .ok()
        .map(|s| parse_watch_dirs(&s))
        .unwrap_or_default();

    // Lead with the version: on a remote box the #1 question when the filter or a
    // fix "doesn't work" is which binary is actually running (an old one silently
    // ignores CSM_WATCH_DIRS entirely). Printing it every start makes a stale binary
    // obvious — and its ABSENCE is itself the tell, since versions before this one
    // don't print it.
    eprintln!(
        "csm-agent: version {} | host={} -> publishing Codex/Claude events to {}/{}",
        env!("CARGO_PKG_VERSION"),
        csm_core::host_name(),
        base.trim_end_matches('/'),
        topic
    );
    if !watch_dirs.is_empty() {
        eprintln!(
            "csm-agent: only relaying sessions under: {}",
            watch_dirs.join(", ")
        );
    }

    let (tx, rx) = mpsc::channel::<Event>();

    // Codex: tail rollouts. Claude: local file bus (if hooks are installed here).
    let tx2 = tx.clone();
    thread::spawn(move || CodexRolloutSource::new(CodexRolloutSource::default_dir()).run(tx));
    thread::spawn(move || FsWatchSource::new(FsWatchSource::default_dir()).run(tx2));

    // Last kind successfully published per session, to coalesce redundant same-kind
    // bursts (see `is_redundant`). Recorded on success only, so a dropped publish is
    // retried on the next event rather than silently swallowed.
    let mut last_published: HashMap<SessionKey, EventKind> = HashMap::new();
    // Pending wait imposed by the relay's rate limit (see `next_backoff`). While set,
    // we pause before each attempt instead of hammering.
    let mut backoff: Option<Duration> = None;
    for ev in rx {
        // Directory whitelist: sessions outside the watched dirs never leave this
        // host (no card, no notification on the desktop).
        if !cwd_allowed(&ev.cwd, &watch_dirs) {
            continue;
        }
        let key = SessionKey::of(&ev);
        if is_redundant(last_published.get(&key).copied(), ev.event) {
            continue; // same visible state as last relayed — don't spam the topic
        }
        if let Some(wait) = backoff {
            thread::sleep(wait); // relay asked us to slow down; honour it
        }
        match ntfy::publish(&base, &topic, token.as_deref(), &ev) {
            Ok(()) => {
                if backoff.take().is_some() {
                    eprintln!("csm-agent: relay recovered; resuming normal publishing");
                }
                if ev.event == EventKind::SessionEnd {
                    last_published.remove(&key); // session gone; a resume starts fresh
                } else {
                    last_published.insert(key, ev.event);
                }
            }
            Err(e) => {
                let next = next_backoff(backoff, matches!(e, PublishError::RateLimited));
                // Log only when the wait changes, so a long outage can't flood the log
                // with one identical line per event (as a 429 storm used to).
                if backoff != Some(next) {
                    eprintln!(
                        "csm-agent: publish failed ({e}); backing off {}s",
                        next.as_secs()
                    );
                }
                backoff = Some(next);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_and_trims_dropping_blanks() {
        assert_eq!(parse_watch_dirs(""), Vec::<String>::new());
        assert_eq!(parse_watch_dirs("  "), Vec::<String>::new());
        assert_eq!(
            parse_watch_dirs("/home/a: /home/b :"),
            vec!["/home/a".to_string(), "/home/b".to_string()]
        );
    }

    #[test]
    fn empty_whitelist_relays_everything() {
        assert!(cwd_allowed("/anything", &[]));
        assert!(cwd_allowed("", &[])); // even a missing cwd, when not filtering
    }

    #[test]
    fn whitelist_keeps_only_in_scope_sessions() {
        let watch = vec!["/home/me/proj".to_string()];
        assert!(cwd_allowed("/home/me/proj", &watch)); // the dir itself
        assert!(cwd_allowed("/home/me/proj/src", &watch)); // a subdir
        assert!(!cwd_allowed("/home/me/other", &watch)); // out of scope
        assert!(!cwd_allowed("/home/me", &watch)); // parent, not under
        assert!(!cwd_allowed("", &watch)); // unknown cwd rejected when filtering
    }

    #[test]
    fn multiple_watched_dirs() {
        let watch = vec!["/home/a".to_string(), "/srv/b".to_string()];
        assert!(cwd_allowed("/home/a/x", &watch));
        assert!(cwd_allowed("/srv/b", &watch));
        assert!(!cwd_allowed("/home/c", &watch));
    }

    #[test]
    fn backoff_starts_at_base_doubles_and_caps() {
        let s = Duration::from_secs;
        assert_eq!(next_backoff(None, true), s(5)); // 429 waits out ntfy's ~1/5s refill
        assert_eq!(next_backoff(None, false), s(1)); // a plain error starts shorter
        assert_eq!(next_backoff(Some(s(5)), true), s(10)); // doubles
        assert_eq!(next_backoff(Some(s(10)), true), s(20));
        assert_eq!(next_backoff(Some(s(40)), true), s(60)); // capped
        assert_eq!(next_backoff(Some(s(60)), true), s(60)); // stays at the cap
        assert_eq!(next_backoff(Some(s(1)), true), s(5)); // 429 never waits below base
    }

    #[test]
    fn redundant_suppresses_only_consecutive_same_kind() {
        use EventKind::*;
        assert!(!is_redundant(None, RunStart)); // first event always publishes
        assert!(is_redundant(Some(RunStart), RunStart)); // per-tool-call spam -> drop
        assert!(is_redundant(Some(WaitingInput), WaitingInput)); // repeated Notification
        assert!(!is_redundant(Some(WaitingInput), RunStart)); // resumed -> state change
        assert!(!is_redundant(Some(RunStart), RunEnd)); // finished -> state change
        assert!(!is_redundant(Some(RunEnd), RunStart)); // new turn -> state change
    }
}
