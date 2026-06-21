//! Claude Code integration: append `session-reporter` hooks to
//! `~/.claude/settings.json` under `UserPromptSubmit`, `PreToolUse`, `Stop`,
//! `Notification`, `SessionEnd`.
//! - `Notification` fires when Claude pauses awaiting the user (permission
//!   prompt / clarifying question) → the widget shows "waiting".
//! - `PreToolUse` fires when Claude runs a tool → clears a stale "waiting" back
//!   to "running" once work resumes (a Notification has no matching resume
//!   event within the same turn).
//!
//! settings.json hook shape we write:
//! ```json
//! { "hooks": { "Stop": [ { "hooks": [ { "type":"command",
//!   "command": "\"<reporter>\" --source claude" } ] } ] } }
//! ```
//! Our entries are identified by the `--source claude` flag in the command, so
//! uninstall removes exactly those and leaves the user's other hooks untouched.

use std::io;
use std::path::Path;

use serde_json::{json, Value};

use super::{backup_file, write_atomic, InstallOutcome};

const EVENTS: [&str; 6] = [
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "Notification",
    "SessionEnd",
];
/// Stable marker identifying a command as ours.
const MARKER: &str = "--source claude";

fn our_command(reporter: &Path) -> String {
    format!("\"{}\" {}", reporter.display(), MARKER)
}

/// Read settings.json into a JSON object. Missing/empty -> `{}`. A parse failure
/// is an error (we must not clobber a config we can't understand).
fn read_settings(path: &Path) -> io::Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => serde_json::from_str(&s).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("settings.json parse error: {e}"),
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e),
    }
}

/// Does this matcher-group contain one of our commands?
fn group_is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|inner| {
            inner.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.contains(MARKER))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn event_has_ours(root: &Value, event: &str) -> bool {
    root.get("hooks")
        .and_then(|h| h.get(event))
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().any(group_is_ours))
        .unwrap_or(false)
}

/// True if our integration is present for all three events.
fn fully_installed(root: &Value) -> bool {
    EVENTS.iter().all(|e| event_has_ours(root, e))
}

pub fn status(config_path: &Path) -> io::Result<InstallOutcome> {
    let root = read_settings(config_path)?;
    let mut out = InstallOutcome::new(config_path);
    out.installed = fully_installed(&root);
    out.summary = if out.installed {
        "Claude Code integration installed".to_string()
    } else {
        "Claude Code integration not installed".to_string()
    };
    Ok(out)
}

pub fn install(config_path: &Path, reporter: &Path) -> io::Result<InstallOutcome> {
    let mut root = read_settings(config_path)?;
    if !root.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "settings.json root is not a JSON object",
        ));
    }
    let cmd = our_command(reporter);
    let mut changed = false;

    let obj = root.as_object_mut().unwrap();
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks_obj = hooks.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "settings.json `hooks` is not an object",
        )
    })?;

    for ev in EVENTS {
        let arr_val = hooks_obj.entry(ev).or_insert_with(|| json!([]));
        let arr = arr_val.as_array_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("settings.json `hooks.{ev}` is not an array"),
            )
        })?;
        if !arr.iter().any(group_is_ours) {
            arr.push(json!({ "hooks": [ { "type": "command", "command": cmd } ] }));
            changed = true;
        }
    }

    let mut out = InstallOutcome::new(config_path);
    out.installed = true;
    if changed {
        out.backup_path = backup_file(config_path)?;
        let text = serde_json::to_string_pretty(&root)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(config_path, &text)?;
        out.changed = true;
        out.summary = format!(
            "Added UserPromptSubmit/PreToolUse/PostToolUse/Stop/Notification/SessionEnd hooks to {} (command: {}). {}",
            config_path.display(),
            cmd,
            out.backup_path
                .as_ref()
                .map(|b| format!("Original backed up to {}", b.display()))
                .unwrap_or_else(|| "(no existing file — created)".to_string())
        );
    } else {
        out.summary =
            "Claude Code integration already present — nothing to do (idempotent)".to_string();
    }
    Ok(out)
}

pub fn uninstall(config_path: &Path) -> io::Result<InstallOutcome> {
    let mut out = InstallOutcome::new(config_path);
    if !config_path.exists() {
        out.summary = "settings.json does not exist — nothing to uninstall".to_string();
        return Ok(out);
    }
    let mut root = read_settings(config_path)?;
    let mut changed = false;

    if let Some(hooks_obj) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for ev in EVENTS {
            if let Some(arr) = hooks_obj.get_mut(ev).and_then(|a| a.as_array_mut()) {
                let before = arr.len();
                arr.retain(|g| !group_is_ours(g));
                if arr.len() != before {
                    changed = true;
                }
            }
        }
        // drop now-empty event arrays
        let empty_events: Vec<String> = hooks_obj
            .iter()
            .filter(|(_, v)| v.as_array().map(|a| a.is_empty()).unwrap_or(false))
            .map(|(k, _)| k.clone())
            .collect();
        for k in empty_events {
            hooks_obj.remove(&k);
        }
        let hooks_empty = hooks_obj.is_empty();
        if hooks_empty {
            root.as_object_mut().unwrap().remove("hooks");
        }
    }

    if changed {
        out.backup_path = backup_file(config_path)?;
        let text = serde_json::to_string_pretty(&root)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(config_path, &text)?;
        out.changed = true;
        out.summary = format!(
            "Removed our hook entries from {}. {}",
            config_path.display(),
            out.backup_path
                .as_ref()
                .map(|b| format!("Original backed up to {}", b.display()))
                .unwrap_or_default()
        );
    } else {
        out.summary = "No hook entries of ours found — no changes".to_string();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn reporter() -> PathBuf {
        PathBuf::from("D:/app/session-reporter.exe")
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn install_into_missing_file_adds_all_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        let out = install(&cfg, &reporter()).unwrap();
        assert!(out.changed && out.installed);
        let v = read(&cfg);
        for ev in EVENTS {
            assert!(event_has_ours(&v, ev), "missing hook for {ev}");
        }
    }

    #[test]
    fn install_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        install(&cfg, &reporter()).unwrap();
        let second = install(&cfg, &reporter()).unwrap();
        assert!(!second.changed, "second install must not change anything");
        // exactly one group per event
        let v = read(&cfg);
        for ev in EVENTS {
            let n = v["hooks"][ev].as_array().unwrap().len();
            assert_eq!(n, 1, "duplicate group for {ev}");
        }
    }

    #[test]
    fn install_preserves_existing_user_config_and_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        // a pre-existing unrelated user hook + other settings
        std::fs::write(
            &cfg,
            r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo user-hook"}]}]}}"#,
        )
        .unwrap();
        install(&cfg, &reporter()).unwrap();
        let v = read(&cfg);
        assert_eq!(v["theme"], "dark", "unrelated settings preserved");
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "user hook kept + ours appended");
        assert!(stop.iter().any(|g| g["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("echo user-hook")));
    }

    #[test]
    fn uninstall_removes_only_ours_and_restores_user_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        std::fs::write(
            &cfg,
            r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo user-hook"}]}]}}"#,
        )
        .unwrap();
        install(&cfg, &reporter()).unwrap();
        let out = uninstall(&cfg).unwrap();
        assert!(out.changed);
        let v = read(&cfg);
        assert_eq!(v["theme"], "dark");
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1, "only user hook remains");
        assert!(stop[0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("echo user-hook"));
        // events that became empty (UserPromptSubmit/SessionEnd) are cleaned up
        assert!(v["hooks"].get("UserPromptSubmit").is_none());
    }

    #[test]
    fn uninstall_on_fresh_install_leaves_clean_object() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        install(&cfg, &reporter()).unwrap();
        uninstall(&cfg).unwrap();
        let v = read(&cfg);
        // no leftover hooks key (everything we added is gone)
        assert!(v.get("hooks").is_none(), "hooks should be fully removed");
    }

    #[test]
    fn status_reflects_install_state() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        assert!(!status(&cfg).unwrap().installed);
        install(&cfg, &reporter()).unwrap();
        assert!(status(&cfg).unwrap().installed);
    }

    #[test]
    fn parse_error_aborts_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        std::fs::write(&cfg, "{ this is not json").unwrap();
        assert!(install(&cfg, &reporter()).is_err());
        // file untouched
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "{ this is not json");
    }
}
