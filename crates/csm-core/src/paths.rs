//! Centralized, cross-platform path & host resolution.
//!
//! This is the single place platform differences live. Reporter, app backend and
//! installer all call these instead of computing paths themselves, so porting to
//! a new OS (or switching to XDG dirs on Linux) touches only this module.

use std::path::PathBuf;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Application data root: `~/.cli-session-monitor`.
///
/// Kept identical on all platforms for simplicity and to match the dotfolder
/// style of the CLIs we integrate with (`~/.claude`, `~/.codex`). A future Linux
/// XDG variant (`~/.local/share/...`) would be a `cfg!`-gated change here only.
pub fn data_dir() -> PathBuf {
    home().join(".cli-session-monitor")
}

/// Event file bus: `~/.cli-session-monitor/events`.
pub fn events_dir() -> PathBuf {
    data_dir().join("events")
}

/// User config: `~/.cli-session-monitor/config.json`.
pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Where the installer writes config backups before modifying CLI configs.
pub fn backups_dir() -> PathBuf {
    data_dir().join("backups")
}

/// Claude Code settings file: `~/.claude/settings.json`.
pub fn claude_settings_path() -> PathBuf {
    home().join(".claude").join("settings.json")
}

/// Codex config file: `~/.codex/config.toml`.
pub fn codex_config_path() -> PathBuf {
    home().join(".codex").join("config.toml")
}

/// Codex per-session rollout directory: `~/.codex/sessions`.
pub fn codex_sessions_dir() -> PathBuf {
    home().join(".codex").join("sessions")
}

/// Claude Code per-session transcript directory: `~/.claude/projects`.
/// Each project subfolder holds `<session-id>.jsonl` transcripts (carry `cwd`).
pub fn claude_projects_dir() -> PathBuf {
    home().join(".claude").join("projects")
}

/// Origin host / device name, cross-platform.
///
/// Uses the OS hostname (not an env var): `COMPUTERNAME`/`HOSTNAME` are not
/// reliably set in non-interactive shells on Linux, which is exactly where the
/// reporter runs (invoked by a hook). Falls back to `"unknown-host"`.
pub fn host_name() -> String {
    let h = gethostname::gethostname().to_string_lossy().into_owned();
    if h.is_empty() {
        "unknown-host".to_string()
    } else {
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_under_data_dir() {
        let d = data_dir();
        assert!(events_dir().starts_with(&d));
        assert!(config_path().starts_with(&d));
        assert!(backups_dir().starts_with(&d));
        assert!(config_path().ends_with("config.json"));
        assert!(events_dir().ends_with("events"));
    }

    #[test]
    fn cli_config_paths_have_expected_tails() {
        assert!(claude_settings_path().ends_with("settings.json"));
        assert!(claude_settings_path().to_string_lossy().contains(".claude"));
        assert!(codex_config_path().ends_with("config.toml"));
        assert!(codex_config_path().to_string_lossy().contains(".codex"));
    }

    #[test]
    fn host_name_is_non_empty() {
        assert!(!host_name().is_empty());
    }
}
