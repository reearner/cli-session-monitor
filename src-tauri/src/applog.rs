//! Tiny best-effort file logger at `~/.cli-session-monitor/csm.log`.
//!
//! Used to diagnose window-matching / discovery without a debugger. Never panics
//! or blocks the app; self-truncates when it grows past ~1 MB.

use std::io::Write;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Logging is off unless `CSM_DEBUG` is set (to anything but empty/`0`), so normal
/// runs write nothing (no disk churn, no path info on disk). Checked once.
pub fn enabled() -> bool {
    static EN: OnceLock<bool> = OnceLock::new();
    *EN.get_or_init(|| {
        std::env::var("CSM_DEBUG")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

fn stamp() -> String {
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // UTC HH:MM:SS — enough for ordering / correlating with actions.
    format!("{:02}:{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60, s % 60)
}

/// Append one timestamped line to the log file (best-effort; no-op unless
/// `CSM_DEBUG` is set).
pub fn line(msg: &str) {
    if !enabled() {
        return;
    }
    let path = csm_core::paths::data_dir().join("csm.log");
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if std::fs::metadata(&path)
        .map(|m| m.len() > 1_000_000)
        .unwrap_or(false)
    {
        let _ = std::fs::write(&path, "");
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{} {}", stamp(), msg);
    }
}
