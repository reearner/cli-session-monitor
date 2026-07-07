//! csm-agent — runs on a (remote) machine where Codex/Claude execute, watches
//! their session files, and publishes normalized events to an ntfy relay topic
//! that the desktop app subscribes to. No SSH required; only metadata is sent.
//!
//! Config via env:
//!   CSM_RELAY_TOPIC  (required)  the ntfy topic to publish to
//!   CSM_RELAY_URL    (optional)  default https://ntfy.sh
//!   CSM_RELAY_TOKEN  (optional)  ntfy access token
//!   CSM_WATCH_DIRS   (optional)  colon-separated whitelist of project dirs; when
//!                                set, ONLY sessions whose cwd is inside one of them
//!                                are relayed (others never leave this host). The
//!                                generated remote-agent.sh sets it via --include-dir.

use std::sync::mpsc;
use std::thread;

use csm_core::Event;
use csm_watch::{ntfy, CodexRolloutSource, FsWatchSource, Source};

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

fn main() {
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

    eprintln!(
        "csm-agent: host={} -> publishing Codex/Claude events to {}/{}",
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

    for ev in rx {
        // Directory whitelist: sessions outside the watched dirs never leave this
        // host (no card, no notification on the desktop).
        if !cwd_allowed(&ev.cwd, &watch_dirs) {
            continue;
        }
        if let Err(e) = ntfy::publish(&base, &topic, token.as_deref(), &ev) {
            eprintln!("csm-agent: publish failed: {e}");
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
}
