use serde::Deserialize;

use csm_core::{Event, EventKind, Source};

use super::{cwd_or_current, host_name, now_ms, AdapterResult};

/// Subset of the Codex `notify` JSON payload.
///
/// We deliberately ignore message-content fields (e.g. `last-assistant-message`)
/// — only metadata is read. Codex has no documented clean "turn started" signal
/// today, so we map turn-completion to [`EventKind::RunEnd`]; the state machine
/// treats a `RunEnd` with no prior `RunStart` as a done session with unreliable
/// timing (requirement Req 2.4).
///
/// VERIFIED against Codex CLI 0.130.0 (capability probe, 2026-06). The real
/// `agent-turn-complete` payload looks like:
/// ```json
/// {"type":"agent-turn-complete","thread-id":"<uuid>","turn-id":"<uuid>",
///  "cwd":"D:\\tmp","client":"codex_exec",
///  "input-messages":[...],"last-assistant-message":"..."}
/// ```
/// We key the session on `thread-id` (stable across turns within one Codex
/// session); `turn-id` changes every turn and is intentionally ignored. The
/// content fields (`input-messages`, `last-assistant-message`) are absent from
/// this struct, so serde drops them — conversation text never enters an Event.
/// Codex emits no clean "turn started" signal, so we only produce `run_end`; the
/// state machine then reports unreliable timing (Req 2.4). Extra `alias`es keep
/// other id spellings working if a future Codex version renames the field.
#[derive(Deserialize)]
struct CodexNotify {
    #[serde(rename = "type", default)]
    kind: String,

    #[serde(default)]
    #[serde(alias = "thread-id")]
    #[serde(alias = "thread_id")]
    #[serde(alias = "conversation-id")]
    #[serde(alias = "conversation_id")]
    #[serde(alias = "session-id")]
    session_id: String,

    #[serde(default)]
    #[serde(alias = "workdir")]
    #[serde(alias = "working-directory")]
    cwd: String,
}

/// Parse a Codex `notify` payload into a normalized [`Event`].
///
/// Turn-completion types map to `RunEnd`; anything else is ignored (`Ok(None)`).
pub fn parse(input: &str) -> AdapterResult {
    let n: CodexNotify = serde_json::from_str(input)?;

    let kind = match n.kind.as_str() {
        "agent-turn-complete" | "turn-complete" | "turn_complete" => EventKind::RunEnd,
        _ => return Ok(None),
    };

    // Without a stable id we fall back to a constant key so repeated Codex turns
    // in the same host collapse onto one card rather than spawning new ones.
    let session_id = if n.session_id.is_empty() {
        "codex-session".to_string()
    } else {
        n.session_id
    };

    Ok(Some(Event::new(
        Source::Codex,
        session_id,
        cwd_or_current(n.cwd),
        host_name(),
        kind,
        now_ms(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_complete_maps_to_run_end() {
        let raw = r#"{"type":"agent-turn-complete","conversation-id":"c1","cwd":"/proj"}"#;
        let ev = parse(raw).unwrap().unwrap();
        assert_eq!(ev.event, EventKind::RunEnd);
        assert_eq!(ev.source, Source::Codex);
        assert_eq!(ev.session_id, "c1");
        assert_eq!(ev.cwd, "/proj");
    }

    #[test]
    fn real_codex_0130_payload_uses_thread_id_and_drops_content() {
        // Exact payload captured from Codex CLI 0.130.0 `agent-turn-complete`.
        let raw = r#"{"type":"agent-turn-complete","thread-id":"019ec1b5-c8e7-7623-a011-b100481655f9","turn-id":"019ec1b5-c9d5-74c2-bb69-700ba194db06","cwd":"D:\\tmp","client":"codex_exec","input-messages":["Reply with exactly the single word: hi"],"last-assistant-message":"hi"}"#;
        let ev = parse(raw).unwrap().unwrap();
        assert_eq!(ev.event, EventKind::RunEnd);
        assert_eq!(ev.source, Source::Codex);
        // session keyed on thread-id (stable), NOT turn-id, NOT the fallback
        assert_eq!(ev.session_id, "019ec1b5-c8e7-7623-a011-b100481655f9");
        assert_eq!(ev.cwd, "D:\\tmp");
        // conversation content must never leak into the event
        let s = serde_json::to_string(&ev).unwrap();
        assert!(!s.contains("last-assistant-message"));
        assert!(!s.contains("input-messages"));
        assert!(!s.contains("Reply with exactly"));
    }

    #[test]
    fn other_type_is_ignored() {
        assert!(parse(r#"{"type":"something-else"}"#).unwrap().is_none());
    }

    #[test]
    fn missing_identity_uses_stable_fallback() {
        let ev = parse(r#"{"type":"agent-turn-complete"}"#).unwrap().unwrap();
        assert_eq!(ev.session_id, "codex-session");
    }

    #[test]
    fn never_captures_message_content() {
        // A content field that happens to be present must not end up in the Event.
        let raw = r#"{"type":"agent-turn-complete","last-assistant-message":"SECRET-CONTENT"}"#;
        let ev = parse(raw).unwrap().unwrap();
        let serialized = serde_json::to_string(&ev).unwrap();
        assert!(!serialized.contains("SECRET-CONTENT"));
    }

    #[test]
    fn bad_json_is_error_not_panic() {
        assert!(parse("}{").is_err());
    }
}
