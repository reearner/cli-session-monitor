//! ntfy-style relay: publish/subscribe `Event`s over an HTTP pub-sub topic.
//!
//! This is the no-SSH path for remote monitoring: a remote `csm-agent` POSTs
//! events to a topic; the local app subscribes to the same topic. (A phone can
//! later subscribe to the same topic in the ntfy app — same mechanism.)
//!
//! Only metadata leaves the machine (the `Event` schema — never conversation
//! content). Use a hard-to-guess topic and/or a self-hosted ntfy for privacy.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use csm_core::Event;

use crate::Source;

type DynErr = Box<dyn std::error::Error>;

fn base_trim(base: &str) -> &str {
    base.trim_end_matches('/')
}

/// Why a publish failed. `RateLimited` (HTTP 429) is called out separately so the
/// caller can back off instead of hammering: ntfy refills a visitor's request
/// bucket slowly (~1 request / 5s), and clients that ignore 429 can get their IP
/// temporarily blocked.
#[derive(Debug)]
pub enum PublishError {
    /// The relay rejected us with HTTP 429 — wait before publishing again.
    RateLimited,
    Other(Box<dyn std::error::Error>),
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PublishError::RateLimited => write!(f, "rate limited by the relay (HTTP 429)"),
            PublishError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PublishError {}

/// Publish one event to `<base>/<topic>` (the POST body is the event JSON).
pub fn publish(
    base: &str,
    topic: &str,
    token: Option<&str>,
    event: &Event,
) -> Result<(), PublishError> {
    let url = format!("{}/{}", base_trim(base), topic);
    let body = serde_json::to_string(event).map_err(|e| PublishError::Other(Box::new(e)))?;
    let mut req = ureq::post(&url).set("X-CSM", "1");
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    match req.send_string(&body) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(429, _)) => Err(PublishError::RateLimited),
        Err(e) => Err(PublishError::Other(Box::new(e))),
    }
}

/// Subscribes to `<base>/<topic>/json` and feeds parsed events to the channel,
/// reconnecting on errors. Remote events carry `host` so the UI can group them.
pub struct NtfySource {
    pub base: String,
    pub topic: String,
    pub token: Option<String>,
    /// ntfy `since` window (e.g. "5m") so reconnecting picks up recent events
    /// instead of only ones published after connecting. None = live only.
    pub since: Option<String>,
    /// Set to true to make the subscription exit (used to switch topics live).
    pub stop: Arc<AtomicBool>,
    /// Reflects whether the subscription stream is currently open, so the UI can
    /// show a "connected" indicator. None = don't track.
    pub connected: Option<Arc<AtomicBool>>,
}

impl NtfySource {
    fn set_connected(&self, v: bool) {
        if let Some(c) = &self.connected {
            c.store(v, Ordering::SeqCst);
        }
    }
}

impl Source for NtfySource {
    fn run(self, tx: Sender<Event>) {
        let mut url = format!("{}/{}/json", base_trim(&self.base), self.topic);
        if let Some(s) = &self.since {
            url.push_str(&format!("?since={s}"));
        }
        loop {
            if self.stop.load(Ordering::SeqCst) {
                self.set_connected(false);
                return;
            }
            if let Err(e) = self.stream(&url, &tx) {
                eprintln!("ntfy: stream error: {e}");
            }
            self.set_connected(false);
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_secs(5)); // reconnect backoff
        }
    }
}

impl NtfySource {
    fn stream(&self, url: &str, tx: &Sender<Event>) -> Result<(), DynErr> {
        // Read timeout > ntfy's ~45s keepalive: healthy connections stay up, but
        // a dead one (or a pending stop) unblocks within the window.
        let agent = ureq::AgentBuilder::new()
            .timeout_read(Duration::from_secs(70))
            .build();
        let mut req = agent.get(url);
        if let Some(t) = &self.token {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        let reader = BufReader::new(req.call()?.into_reader());
        self.set_connected(true); // stream opened
        for line in reader.lines() {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // ntfy /json stream: one JSON object per line; only `event:"message"`
            // carries a payload, in the `message` field (our Event JSON).
            let v: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("event").and_then(|e| e.as_str()) != Some("message") {
                continue;
            }
            if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
                if let Ok(ev) = serde_json::from_str::<Event>(msg) {
                    let _ = tx.send(ev);
                }
            }
        }
        Ok(())
    }
}
