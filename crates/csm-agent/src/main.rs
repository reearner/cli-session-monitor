//! csm-agent — runs on a (remote) machine where Codex/Claude execute, watches
//! their session files, and publishes normalized events to an ntfy relay topic
//! that the desktop app subscribes to. No SSH required; only metadata is sent.
//!
//! Config via env:
//!   CSM_RELAY_TOPIC  (required)  the ntfy topic to publish to
//!   CSM_RELAY_URL    (optional)  default https://ntfy.sh
//!   CSM_RELAY_TOKEN  (optional)  ntfy access token

use std::sync::mpsc;
use std::thread;

use csm_core::Event;
use csm_watch::{ntfy, CodexRolloutSource, FsWatchSource, Source};

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

    eprintln!(
        "csm-agent: host={} -> publishing Codex/Claude events to {}/{}",
        csm_core::host_name(),
        base.trim_end_matches('/'),
        topic
    );

    let (tx, rx) = mpsc::channel::<Event>();

    // Codex: tail rollouts. Claude: local file bus (if hooks are installed here).
    let tx2 = tx.clone();
    thread::spawn(move || CodexRolloutSource::new(CodexRolloutSource::default_dir()).run(tx));
    thread::spawn(move || FsWatchSource::new(FsWatchSource::default_dir()).run(tx2));

    for ev in rx {
        if let Err(e) = ntfy::publish(&base, &topic, token.as_deref(), &ev) {
            eprintln!("csm-agent: publish failed: {e}");
        }
    }
}
