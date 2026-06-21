use serde::Deserialize;

use csm_core::{Event, EventKind, Source};

use super::{cwd_or_current, host_name, now_ms, AdapterResult};

/// Subset of the Claude Code hook payload (delivered as JSON on stdin) that we
/// need. Unknown fields are ignored.
///
/// NOTE: field names follow Claude Code's documented hook stdin JSON
/// (`session_id`, `cwd`, `hook_event_name`). They must be re-verified against a
/// live session — that end-to-end check is deferred per the testing agreement;
/// these adapters are covered by sample-payload unit tests in the meantime.
#[derive(Deserialize)]
struct ClaudeHook {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    hook_event_name: String,
}

/// Parse a Claude Code hook stdin payload into a normalized [`Event`].
///
/// Mapping:
/// - `UserPromptSubmit -> RunStart` (a new turn begins)
/// - `PreToolUse` / `PostToolUse -> RunStart` (work resumed — clears a stale
///   "waiting" set by a Notification. PostToolUse fires the instant an
///   AskUserQuestion tool returns, i.e. right when the user answers, so the card
///   flips back to running immediately even before Claude's next tool.)
/// - `Notification -> WaitingInput` (Claude paused mid-turn needing the user — a
///   permission prompt, a clarifying question, or idle awaiting input)
/// - `Stop -> RunEnd`
/// - `SessionEnd -> SessionEnd`
///
/// Any other hook is ignored (`Ok(None)`).
pub fn parse(input: &str) -> AdapterResult {
    let hook: ClaudeHook = serde_json::from_str(input)?;

    let kind = match hook.hook_event_name.as_str() {
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => EventKind::RunStart,
        "Stop" => EventKind::RunEnd,
        "Notification" => EventKind::WaitingInput,
        "SessionEnd" => EventKind::SessionEnd,
        _ => return Ok(None),
    };

    Ok(Some(Event::new(
        Source::ClaudeCode,
        hook.session_id,
        cwd_or_current(hook.cwd),
        host_name(),
        kind,
        now_ms(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_user_prompt_submit_to_run_start() {
        let raw = r#"{"session_id":"abc","cwd":"/proj","hook_event_name":"UserPromptSubmit"}"#;
        let ev = parse(raw).unwrap().unwrap();
        assert_eq!(ev.event, EventKind::RunStart);
        assert_eq!(ev.session_id, "abc");
        assert_eq!(ev.cwd, "/proj");
        assert_eq!(ev.source, Source::ClaudeCode);
    }

    #[test]
    fn maps_stop_to_run_end() {
        let ev = parse(r#"{"session_id":"a","cwd":"/p","hook_event_name":"Stop"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(ev.event, EventKind::RunEnd);
    }

    #[test]
    fn maps_session_end() {
        let ev = parse(r#"{"session_id":"a","cwd":"/p","hook_event_name":"SessionEnd"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(ev.event, EventKind::SessionEnd);
    }

    #[test]
    fn maps_pre_tool_use_to_run_start() {
        let ev = parse(r#"{"session_id":"a","cwd":"/p","hook_event_name":"PreToolUse"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(ev.event, EventKind::RunStart);
    }

    #[test]
    fn maps_post_tool_use_to_run_start() {
        let ev = parse(r#"{"session_id":"a","cwd":"/p","hook_event_name":"PostToolUse"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(ev.event, EventKind::RunStart);
    }

    #[test]
    fn maps_notification_to_waiting_input() {
        let ev = parse(r#"{"session_id":"a","cwd":"/p","hook_event_name":"Notification"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(ev.event, EventKind::WaitingInput);
    }

    #[test]
    fn unknown_hook_is_ignored() {
        let ev = parse(r#"{"session_id":"a","cwd":"/p","hook_event_name":"PreCompact"}"#).unwrap();
        assert!(ev.is_none());
    }

    #[test]
    fn missing_fields_do_not_panic() {
        let ev = parse(r#"{"hook_event_name":"Stop"}"#).unwrap().unwrap();
        assert_eq!(ev.event, EventKind::RunEnd);
        assert_eq!(ev.session_id, ""); // empty but safe; never panics
    }

    #[test]
    fn bad_json_is_error_not_panic() {
        assert!(parse("not json at all").is_err());
    }
}
