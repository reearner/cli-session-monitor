//! The non-negotiable red line: `session-reporter` must always exit 0, even on
//! bad input or an unknown source, so it can never break the calling CLI.
//!
//! These cases use error / no-op paths only, so they never write a real event
//! file into the user's home directory.

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_session-reporter")
}

#[test]
fn malformed_stdin_exits_zero() {
    let mut child = Command::new(bin())
        .arg("--source")
        .arg("claude")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn reporter");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"this is not json")
        .unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "malformed stdin must still exit 0");
}

#[test]
fn unknown_source_exits_zero() {
    let status = Command::new(bin())
        .arg("--source")
        .arg("bogus")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "unknown source must exit 0");
}

#[test]
fn no_args_exits_zero() {
    let status = Command::new(bin())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "no args must exit 0");
}

#[test]
fn empty_stdin_exits_zero() {
    let status = Command::new(bin())
        .arg("--source")
        .arg("codex")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "empty input must exit 0");
}
