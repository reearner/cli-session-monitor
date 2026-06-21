//! `session-reporter` — invoked by Claude Code hooks / Codex notify to record a
//! normalized lifecycle event onto the local file bus.
//!
//! HARD RULE (the highest priority of this whole project): this program must
//! NEVER block or break the calling CLI. Every path:
//!   * is wrapped so any error is swallowed,
//!   * bounds the stdin read with a short timeout,
//!   * and **always exits with code 0**.
//!
//! If the desktop app isn't running, events simply accumulate on disk and are
//! drained later — that is not an error here.

mod adapter;
mod sink;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use csm_core::Event;
use sink::{FileSink, Sink};

/// Upper bound on how long we'll wait for stdin before giving up. Hooks close
/// stdin promptly, so this only guards against a misbehaving caller.
const STDIN_TIMEOUT_MS: u64 = 2000;

fn main() -> ExitCode {
    // Admin subcommands (run manually, e.g. by the remote-agent launcher) — these
    // DO report success/failure, unlike the hook path which always exits 0.
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--install" => return admin_install(),
            "--uninstall" => return admin_uninstall(),
            _ => {}
        }
    }
    // Swallow everything: the calling CLI must never observe a failure here.
    let _ = run();
    ExitCode::SUCCESS
}

/// Claude settings.json to edit. `CSM_CLAUDE_SETTINGS` overrides it (used to test
/// install/uninstall against a throwaway file instead of the real ~/.claude one).
fn claude_settings_path() -> PathBuf {
    std::env::var_os("CSM_CLAUDE_SETTINGS")
        .map(PathBuf::from)
        .unwrap_or_else(csm_core::paths::claude_settings_path)
}

/// Install Claude Code hooks pointing at THIS binary (append-only, backup-first,
/// reversible). For setting up Claude monitoring on a remote host.
fn admin_install() -> ExitCode {
    let reporter = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("session-reporter"));
    match csm_core::installer::claude::install(&claude_settings_path(), &reporter) {
        Ok(o) => {
            println!("{}", o.summary);
            if let Some(c) = o.conflict {
                eprintln!("conflict: {c}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("install failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Remove the Claude Code hooks this tool installed (leaves the user's own hooks).
fn admin_uninstall() -> ExitCode {
    match csm_core::installer::claude::uninstall(&claude_settings_path()) {
        Ok(o) => {
            println!("{}", o.summary);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("uninstall failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(source) = parse_source_arg() else {
        // Unknown / missing --source: nothing to do, but still a clean exit.
        return Ok(());
    };

    let input = read_input();
    if input.trim().is_empty() {
        return Ok(());
    }

    let event: Option<Event> = match source {
        SourceArg::Claude => adapter::claude::parse(&input)?,
        SourceArg::Codex => adapter::codex::parse(&input)?,
    };

    if let Some(event) = event {
        let sink = FileSink::new(events_dir())?;
        sink.emit(&event)?;
    }
    Ok(())
}

enum SourceArg {
    Claude,
    Codex,
}

/// Parse `--source claude|codex` from argv.
fn parse_source_arg() -> Option<SourceArg> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--source" {
            return match args.next().as_deref() {
                Some("claude") => Some(SourceArg::Claude),
                Some("codex") => Some(SourceArg::Codex),
                _ => None,
            };
        }
    }
    None
}

/// Obtain the native payload. Claude delivers JSON on stdin; Codex passes JSON as
/// a trailing argv entry. We accept either: prefer a trailing `{...}` arg, else
/// read stdin (time-bounded).
fn read_input() -> String {
    if let Some(arg) = trailing_json_arg() {
        return arg;
    }
    read_stdin_timeout(STDIN_TIMEOUT_MS)
}

fn trailing_json_arg() -> Option<String> {
    std::env::args()
        .skip(1)
        .rev()
        .find(|a| a.trim_start().starts_with('{'))
}

/// Read stdin to end, but never block longer than `ms`. The reader thread is
/// detached; if it never completes the process still exits cleanly.
fn read_stdin_timeout(ms: u64) -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    rx.recv_timeout(Duration::from_millis(ms))
        .unwrap_or_default()
}

/// `~/.cli-session-monitor/events` (resolved by the shared `csm_core::paths`).
fn events_dir() -> PathBuf {
    csm_core::paths::events_dir()
}
