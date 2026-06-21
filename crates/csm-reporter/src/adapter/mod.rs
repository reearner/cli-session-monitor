//! Per-CLI adapters that parse a native payload and normalize it into [`Event`].
//!
//! Each adapter is a pure function `&str -> AdapterResult` so it can be unit
//! tested with sample payloads and **without any CLI installed**.

use csm_core::Event;

pub mod claude;
pub mod codex;

/// `Ok(None)` means "a valid payload we intentionally ignore" (e.g. a hook event
/// kind we don't track) — the caller should still exit 0 without writing.
pub type AdapterResult = Result<Option<Event>, Box<dyn std::error::Error>>;

/// Origin host name — cross-platform, via `csm_core` (uses the OS hostname, not
/// env vars, so it works under non-interactive hooks on Linux too).
pub use csm_core::host_name;

/// Current time in epoch milliseconds (0 on the impossible pre-epoch case).
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Fall back to the process CWD when the payload omits a working directory.
pub(crate) fn cwd_or_current(cwd: String) -> String {
    if cwd.is_empty() {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    } else {
        cwd
    }
}
