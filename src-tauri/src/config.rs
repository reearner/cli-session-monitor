//! User configuration (`~/.cli-session-monitor/config.json`).
//!
//! A missing or corrupt file always degrades to defaults rather than failing —
//! the app must start regardless of config state.

use std::collections::HashMap;
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

    /// How many days back to keep surfacing on-disk LOCAL sessions as (idle)
    /// cards, so recurring sessions stay around. Min 1.
    #[serde(default = "default_discover_days")]
    pub discover_window_days: u32,

    /// User-assigned card names, keyed by session id, so a renamed card keeps its
    /// name across restarts and `--resume`. The user types these — they are NOT
    /// read from conversation content (preserving the metadata-only promise).
    #[serde(default)]
    pub session_names: HashMap<String, String>,

    /// User-edited resume commands, keyed by session id. Lets a card remember the
    /// exact command to relaunch it (e.g. with `--yolo` /
    /// `--dangerously-skip-permissions`), which the copy button would otherwise
    /// drop. Empty/absent = use the auto-generated default.
    #[serde(default)]
    pub session_cmds: HashMap<String, String>,
}

fn default_discover_days() -> u32 {
    3
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
    3600
}

impl Default for Config {
    fn default() -> Self {
        Self {
            notifications: true,
            sound: true,
            idle_threshold_secs: default_idle_secs(),
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
            discover_window_days: default_discover_days(),
            session_names: HashMap::new(),
            session_cmds: HashMap::new(),
        }
    }
}

/// Keep only the newest `keep` `config-*.json` files in `dir` (backups are named
/// `config-<epoch_millis>.json`, so lexical order == chronological order).
fn prune_backups(dir: &Path, keep: usize) {
    let mut backups: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("config-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return,
    };
    if backups.len() <= keep {
        return;
    }
    backups.sort();
    let excess = backups.len() - keep;
    for old in backups.into_iter().take(excess) {
        let _ = std::fs::remove_file(old);
    }
}

impl Config {
    /// Read config from `path`, falling back to [`Config::default`] if the file is
    /// missing or unparseable. Never errors.
    pub fn load_from(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(text) => {
                // Tolerate a leading UTF-8 BOM (e.g. a file written by PowerShell's
                // `Set-Content -Encoding utf8`). serde_json rejects a BOM, which
                // would silently reset the WHOLE config — and the next save would
                // then overwrite the user's real data with defaults.
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                serde_json::from_str(text).unwrap_or_else(|err| {
                    eprintln!("config: parse failed ({err}); using defaults");
                    // Archive the unparseable file first, so its data isn't lost
                    // when defaults are later saved over it.
                    if let Some(parent) = path.parent() {
                        let backups = parent.join("backups");
                        if std::fs::create_dir_all(&backups).is_ok() {
                            let stamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis())
                                .unwrap_or(0);
                            let _ = std::fs::copy(
                                path,
                                backups.join(format!("config-unparsed-{stamp}.json")),
                            );
                        }
                    }
                    Config::default()
                })
            }
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

    /// Like [`save_to`], but first archives the CURRENT on-disk config (if any) to
    /// a timestamped file under `backups/`, keeping the newest few. Use this for
    /// MEANINGFUL edits (settings, session names, remembered commands) so a bad
    /// write can never silently lose the old data. Positional saves
    /// (`save_window_pos/size`) intentionally use plain `save_to` to avoid churn.
    pub fn save_with_backup(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        if path.exists() {
            if let Some(parent) = path.parent() {
                let backups = parent.join("backups");
                if std::fs::create_dir_all(&backups).is_ok() {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let _ = std::fs::copy(path, backups.join(format!("config-{stamp}.json")));
                    prune_backups(&backups, 10);
                }
            }
        }
        self.save_to(path)
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
            discover_window_days: 7,
            session_names: HashMap::from([("sid-123".to_string(), "My agent".to_string())]),
            session_cmds: HashMap::from([(
                "sid-123".to_string(),
                "codex resume --yolo sid-123".to_string(),
            )]),
        };
        cfg.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), cfg);
    }

    #[test]
    fn leading_utf8_bom_is_tolerated() {
        // A config written by PowerShell (`Set-Content -Encoding utf8`) starts with
        // a UTF-8 BOM; it must still parse, not silently reset to defaults.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut cfg = Config::default();
        cfg.session_names
            .insert("id".to_string(), "kept".to_string());
        let json = format!("\u{feff}{}", serde_json::to_string(&cfg).unwrap());
        std::fs::write(&path, json).unwrap();
        let loaded = Config::load_from(&path);
        assert_eq!(
            loaded.session_names.get("id").map(String::as_str),
            Some("kept")
        );
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
        assert_eq!(cfg.idle_threshold_secs, 3600); // default (1 hour)
        assert!(cfg.always_on_top); // default
    }

    #[test]
    fn save_with_backup_archives_previous_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // First write (no prior file → no backup yet).
        Config::default().save_with_backup(&path).unwrap();
        assert!(path.exists());
        let backups = dir.path().join("backups");
        // Many subsequent saves should archive the previous file and cap the count.
        for i in 0..15 {
            let mut cfg = Config::default();
            cfg.panel_w = 300 + i; // change content each time
            cfg.save_with_backup(&path).unwrap();
        }
        let count = std::fs::read_dir(&backups).unwrap().count();
        assert!(count > 0, "a previous config should have been archived");
        assert!(count <= 10, "backups are pruned to the newest 10, got {count}");
    }

    #[test]
    fn save_creates_missing_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("config.json");
        Config::default().save_to(&path).unwrap();
        assert!(path.exists());
    }
}
