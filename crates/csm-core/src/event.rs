use serde::{Deserialize, Serialize};

/// Schema version embedded in every event. Bump on breaking changes so consumers
/// can detect and skip incompatible payloads instead of mis-parsing them.
pub const SCHEMA_VERSION: u32 = 1;

/// Which CLI produced the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
}

/// Normalized lifecycle event kind (CLI-agnostic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    /// A turn started — the model began processing user input. Starts the timer.
    #[serde(rename = "run_start")]
    RunStart,
    /// A turn finished — the model is done replying. Stops the timer.
    #[serde(rename = "run_end")]
    RunEnd,
    /// The session paused mid-turn awaiting user input/approval (e.g. Codex
    /// asking to run a command). Keeps the timer; marks the session as waiting.
    #[serde(rename = "waiting_input")]
    WaitingInput,
    /// The session ended — the card should be removed.
    #[serde(rename = "session_end")]
    SessionEnd,
    /// A pre-existing session found on disk (Claude/Codex session file) that the
    /// app didn't see start. Shown as idle so it can be jumped to; never
    /// overrides a session that's already active.
    #[serde(rename = "discovered")]
    Discovered,
}

/// A single normalized event written to (and read from) the file bus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Equals [`SCHEMA_VERSION`] at write time.
    pub schema: u32,
    pub source: Source,
    pub session_id: String,
    /// Project directory the session is working in (remote path on remote hosts).
    pub cwd: String,
    /// Origin host / device identifier. Local events use the machine name; remote
    /// (Phase 2) events carry the remote host so sessions can be grouped.
    pub host: String,
    pub event: EventKind,
    /// Event time, epoch milliseconds.
    pub ts: i64,
}

impl Event {
    /// Construct an event, stamping the current [`SCHEMA_VERSION`].
    pub fn new(
        source: Source,
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        host: impl Into<String>,
        event: EventKind,
        ts: i64,
    ) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            source,
            session_id: session_id.into(),
            cwd: cwd.into(),
            host: host.into(),
            event,
            ts,
        }
    }
}

/// Identity of a session across sources and hosts.
///
/// `cwd` is intentionally **not** part of the identity (a session keeps its key
/// even if it reports a different directory); `host` **is**, so the same
/// `session_id` on two machines never collides — this is what lets remote
/// sessions (Phase 2) coexist with local ones.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey {
    pub source: Source,
    pub host: String,
    pub session_id: String,
}

impl SessionKey {
    /// Derive the identity key from an event.
    pub fn of(event: &Event) -> Self {
        Self {
            source: event.source,
            host: event.host.clone(),
            session_id: event.session_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_json_roundtrip() {
        let ev = Event::new(
            Source::ClaudeCode,
            "s1",
            "/proj",
            "host1",
            EventKind::RunStart,
            1_700_000_000_000,
        );
        let json = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn enum_values_use_documented_strings() {
        let ev = Event::new(Source::Codex, "s", "/c", "h", EventKind::RunEnd, 1);
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["source"], "codex");
        assert_eq!(v["event"], "run_end");
        assert_eq!(v["schema"], SCHEMA_VERSION);
        assert_eq!(v["ts"], 1);
    }

    #[test]
    fn deserialize_known_payload() {
        let raw = r#"{"schema":1,"source":"claude-code","session_id":"x","cwd":"/p","host":"h","event":"session_end","ts":42}"#;
        let ev: Event = serde_json::from_str(raw).unwrap();
        assert_eq!(ev.source, Source::ClaudeCode);
        assert_eq!(ev.event, EventKind::SessionEnd);
        assert_eq!(ev.ts, 42);
    }

    #[test]
    fn unknown_enum_is_error_not_panic() {
        // A future/foreign source value must fail to parse (so the consumer can
        // skip the file) rather than panic.
        let raw = r#"{"schema":1,"source":"future-cli","session_id":"x","cwd":"/p","host":"h","event":"run_end","ts":1}"#;
        let res: Result<Event, _> = serde_json::from_str(raw);
        assert!(res.is_err());
    }

    #[test]
    fn session_key_ignores_cwd_but_uses_host() {
        let a = Event::new(Source::ClaudeCode, "s", "/c", "h1", EventKind::RunStart, 1);
        let b = Event::new(
            Source::ClaudeCode,
            "s",
            "/other",
            "h1",
            EventKind::RunEnd,
            2,
        );
        let c = Event::new(Source::ClaudeCode, "s", "/c", "h2", EventKind::RunStart, 1);
        assert_eq!(SessionKey::of(&a), SessionKey::of(&b)); // cwd not part of identity
        assert_ne!(SessionKey::of(&a), SessionKey::of(&c)); // host is part of identity
    }
}
