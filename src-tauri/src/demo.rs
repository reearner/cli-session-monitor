//! Demo mode (`CSM_DEMO`): feed a spread of synthetic sessions so the UI can be
//! shown or recorded without any real data. When on, it replaces the real event
//! sources / relay / discovery, `local_host` reports a fake machine name, and
//! per-card window titles are synthesized — so a screenshot or GIF never leaks a
//! real path, hostname, or window.

use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use csm_core::{Event, EventKind, Source};

/// Fake local hostname shown in demo (keeps the real machine name out of GIFs).
pub const HOST: &str = "DESKTOP-DEV";

/// On when `CSM_DEMO` is set to anything but empty / `0`. Checked once.
pub fn enabled() -> bool {
    static EN: OnceLock<bool> = OnceLock::new();
    *EN.get_or_init(|| {
        std::env::var("CSM_DEMO")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn ev(source: Source, id: &str, cwd: &str, host: &str, kind: EventKind, ts: i64) -> Event {
    Event::new(source, id, cwd, host, kind, ts)
}

/// Seed a spread of sessions (running / waiting / done, plus a remote one), then
/// animate a few transitions on a loop so a recording shows live timers, a
/// completion flash, and a waiting flash.
pub fn run(tx: Sender<Event>) {
    let now = now_ms();
    // Local-session cwds live under this base. Default is a clean fake path; set
    // CSM_DEMO_DIR to a real folder (with web-app/api-server/ml-pipeline opened in
    // Cursor/VS Code) so click-to-jump and the highlight actually work on stage.
    let base = std::env::var("CSM_DEMO_DIR").unwrap_or_else(|_| "D:\\projects".to_string());
    let sep = if base.contains('/') { "/" } else { "\\" };
    let web = format!("{base}{sep}web-app");
    let api = format!("{base}{sep}api-server");
    let ml = format!("{base}{sep}ml-pipeline");

    // Backdated timestamps: running timers already show elapsed time, and startup
    // fires no notification (only near-now events count as "fresh").
    let seed = [
        // web-app: running for a few minutes
        (
            Source::Codex,
            "demo-web",
            web.as_str(),
            HOST,
            EventKind::RunStart,
            now - 220_000,
        ),
        // api-server: started, now waiting for input
        (
            Source::ClaudeCode,
            "demo-api",
            api.as_str(),
            HOST,
            EventKind::RunStart,
            now - 95_000,
        ),
        (
            Source::ClaudeCode,
            "demo-api",
            api.as_str(),
            HOST,
            EventKind::WaitingInput,
            now - 60_000,
        ),
        // ml-pipeline: just finished a turn
        (
            Source::ClaudeCode,
            "demo-ml",
            ml.as_str(),
            HOST,
            EventKind::RunStart,
            now - 130_000,
        ),
        (
            Source::ClaudeCode,
            "demo-ml",
            ml.as_str(),
            HOST,
            EventKind::RunEnd,
            now - 40_000,
        ),
        // docs-site on a remote build host: running
        (
            Source::Codex,
            "demo-docs",
            "/srv/docs-site",
            "build-server",
            EventKind::RunStart,
            now - 75_000,
        ),
    ];
    for (src, id, cwd, host, kind, ts) in seed {
        let _ = tx.send(ev(src, id, cwd, host, kind, ts));
    }

    // Animate, so the panel looks alive and flashes often (tuned tight so a
    // screen recording catches a flash within a few seconds).
    loop {
        thread::sleep(Duration::from_secs(5));
        // ml-pipeline runs a new turn, then completes -> amber flash.
        let _ = tx.send(ev(
            Source::ClaudeCode,
            "demo-ml",
            &ml,
            HOST,
            EventKind::RunStart,
            now_ms(),
        ));
        thread::sleep(Duration::from_secs(3));
        let _ = tx.send(ev(
            Source::ClaudeCode,
            "demo-ml",
            &ml,
            HOST,
            EventKind::RunEnd,
            now_ms(),
        ));
        thread::sleep(Duration::from_secs(5));
        // api-server toggles waiting <-> running.
        let _ = tx.send(ev(
            Source::ClaudeCode,
            "demo-api",
            &api,
            HOST,
            EventKind::WaitingInput,
            now_ms(),
        ));
        thread::sleep(Duration::from_secs(3));
        let _ = tx.send(ev(
            Source::ClaudeCode,
            "demo-api",
            &api,
            HOST,
            EventKind::RunStart,
            now_ms(),
        ));
    }
}

/// A synthesized "matched window" title for a demo session's cwd, so cards show a
/// plausible 🪟 line instead of a "no matching window" note. Local sessions look
/// like an open Cursor window; the remote one is labeled via Remote-SSH.
pub fn window_title(cwd: &str, host: &str) -> Option<String> {
    let sep = |c: char| c == '\\' || c == '/';
    let base = cwd
        .trim_end_matches(sep)
        .rsplit(sep)
        .next()
        .filter(|s| !s.is_empty())?;
    if host.is_empty() || host == HOST {
        Some(format!("{base} - Cursor"))
    } else {
        Some(format!("{base} [SSH: {host}] - Cursor"))
    }
}
