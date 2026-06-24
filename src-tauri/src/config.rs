//! User configuration (`~/.cli-session-monitor/config.json`).
//!
//! A missing or corrupt file always degrades to defaults rather than failing —
//! the app must start regardless of config state.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Show a desktop notification when a session finishes.
    #[serde(default = "default_true")]
    pub notifications: bool,
    /// Play a sound when a session finishes.
    #[serde(default = "default_true")]
    pub sound: bool,
    /// Seconds a `done` session stays before it is dimmed to `idle`.
    #[serde(default = "default_idle_secs")]
    pub idle_threshold_secs: u32,
    /// Keep the widget above other windows.
    #[serde(default = "default_true")]
    pub always_on_top: bool,

    // ---- Desktop-resident options (all default OFF, user-toggleable) ----
    /// Launch automatically on login and stay resident.
    #[serde(default)]
    pub autostart: bool,
    /// Dock the window to the left edge of the screen on start.
    #[serde(default)]
    pub dock_left: bool,
    /// Pin to the desktop (sit below other windows) instead of floating on top.
    /// When true this overrides `always_on_top`.
    #[serde(default)]
    pub desktop_pinned: bool,
    /// Hide from the taskbar / Alt-Tab (manage via the tray only).
    #[serde(default)]
    pub skip_taskbar: bool,
    /// Lightweight mode: collapse to a small pill that flashes on completion /
    /// waiting; click to expand.
    #[serde(default)]
    pub lightweight: bool,

    // ---- Remote relay (ntfy): subscribe to a topic to see remote sessions ----
    /// ntfy base URL. Default public ntfy.sh; can point to a self-hosted ntfy.
    #[serde(default = "default_relay_url")]
    pub relay_url: String,
    /// ntfy topic to subscribe to. Empty = remote monitoring disabled.
    #[serde(default)]
    pub relay_topic: String,
    /// Optional ntfy access token.
    #[serde(default)]
    pub relay_token: String,

    // ---- Persisted widget position (physical px) so it reappears where it was
    // last docked/left across restarts. None = use the configured default. ----
    #[serde(default)]
    pub win_x: Option<i32>,
    #[serde(default)]
    pub win_y: Option<i32>,

    /// UI language: "auto" (follow OS/browser locale), "en", or "zh".
    #[serde(default = "default_language")]
    pub language: String,

    // ---- Full-panel size (logical px), so a user-resized panel is remembered. ----
    #[serde(default = "default_panel_w")]
    pub panel_w: u32,
    #[serde(default = "default_panel_h")]
    pub panel_h: u32,

    /// Whether first-run onboarding (auto-open Settings once) has happened.
    #[serde(default)]
    pub onboarded: bool,
}

fn default_panel_w() -> u32 {
    360
}

fn default_panel_h() -> u32 {
    640
}

fn default_relay_url() -> String {
    "https://ntfy.sh".to_string()
}

fn default_language() -> String {
    "auto".to_string()
}

fn default_true() -> bool {
    true
}

fn default_idle_secs() -> u32 {
    120
}

impl Default for Config {
    fn default() -> Self {
        Self {
            notifications: true,
            sound: true,
            idle_threshold_secs: 120,
            always_on_top: true,
            autostart: false,
            dock_left: false,
            desktop_pinned: false,
            skip_taskbar: false,
            lightweight: false,
            relay_url: default_relay_url(),
            relay_topic: String::new(),
            relay_token: String::new(),
            win_x: None,
            win_y: None,
            language: default_language(),
            panel_w: default_panel_w(),
            panel_h: default_panel_h(),
            onboarded: false,
        }
    }
}

impl Config {
    /// Read config from `path`, falling back to [`Config::default`] if the file is
    /// missing or unparseable. Never errors.
    pub fn load_from(path: impl AsRef<Path>) -> Self {
        match std::fs::read_to_string(path.as_ref()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                eprintln!("config: parse failed ({err}); using defaults");
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    /// Write config to `path`, creating parent directories as needed.
    pub fn save_to(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// `~/.cli-session-monitor/config.json` (via shared `csm_core::paths`).
    pub fn default_path() -> PathBuf {
        csm_core::paths::config_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load_from(dir.path().join("nope.json"));
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let cfg = Config {
            notifications: false,
            sound: true,
            idle_threshold_secs: 30,
            always_on_top: false,
            autostart: true,
            dock_left: true,
            desktop_pinned: true,
            skip_taskbar: true,
            lightweight: true,
            relay_url: "https://ntfy.sh".to_string(),
            relay_topic: "abc".to_string(),
            relay_token: String::new(),
            win_x: Some(100),
            win_y: Some(200),
            language: "zh".to_string(),
            panel_w: 400,
            panel_h: 700,
            onboarded: true,
        };
        cfg.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), cfg);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not valid json").unwrap();
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn partial_json_fills_missing_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"notifications":false}"#).unwrap();
        let cfg = Config::load_from(&path);
        assert!(!cfg.notifications);
        assert!(cfg.sound); // default
        assert_eq!(cfg.idle_threshold_secs, 120); // default
        assert!(cfg.always_on_top); // default
    }

    #[test]
    fn save_creates_missing_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("config.json");
        Config::default().save_to(&path).unwrap();
        assert!(path.exists());
    }
}
