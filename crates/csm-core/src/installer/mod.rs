//! One-click integration installers for Claude Code and Codex.
//!
//! Safety red lines (shared by both):
//! - **Append-only**: never overwrite or drop the user's existing config.
//! - **Backup first**: a timestamped `.bak` is written next to the file before
//!   any modification.
//! - **Idempotent**: installing twice does not duplicate entries.
//! - **Reversible**: uninstall removes exactly what we added, nothing else.
//! - **Abort on bad input**: if the existing config can't be parsed, we stop and
//!   report instead of clobbering it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub mod claude;
pub mod codex;

/// Result of an install/uninstall/status operation, surfaced to the UI so the
/// user can see exactly what changed and where the backup is (Req 6.6).
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallOutcome {
    pub config_path: PathBuf,
    /// Whether the config file was modified by this call.
    pub changed: bool,
    /// Our integration is present after this call.
    pub installed: bool,
    /// Set when a *foreign* configuration blocks us (e.g. Codex `notify` already
    /// points elsewhere) — we never overwrite it; the user decides.
    pub conflict: Option<String>,
    pub backup_path: Option<PathBuf>,
    pub summary: String,
}

impl InstallOutcome {
    fn new(config_path: &Path) -> Self {
        Self {
            config_path: config_path.to_path_buf(),
            changed: false,
            installed: false,
            conflict: None,
            backup_path: None,
            summary: String::new(),
        }
    }
}

/// Resolve the bundled `session-reporter` executable (expected next to the app
/// binary). Used by the default-path Tauri commands.
pub fn reporter_path() -> PathBuf {
    let exe_name = if cfg!(windows) {
        "session-reporter.exe"
    } else {
        "session-reporter"
    };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(exe_name)))
        .unwrap_or_else(|| PathBuf::from(exe_name))
}

fn epoch_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Copy `path` to `<path>.bak.<ts>` next to it. Returns the backup path, or
/// `None` if the file did not exist yet.
fn backup_file(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let backup = path.with_file_name(format!("{}.bak.{}", name, epoch_millis()));
    fs::copy(path, &backup)?;
    Ok(Some(backup))
}

/// Write `contents` to `path` atomically (temp file in the same dir + rename),
/// creating parent directories as needed, so a failed write never leaves a
/// half-written config.
fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp{}",
        path.extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default(),
        epoch_millis()
    ));
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(backup_file(&dir.path().join("nope.json"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn backup_copies_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("c.json");
        fs::write(&f, "original").unwrap();
        let b = backup_file(&f).unwrap().unwrap();
        assert_eq!(fs::read_to_string(&b).unwrap(), "original");
        assert!(f.exists(), "original is left in place");
    }

    #[test]
    fn write_atomic_creates_parent_and_no_tmp_residue() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a").join("b").join("settings.json");
        write_atomic(&f, "{}").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "{}");
        // only the target file exists in its dir
        let count = fs::read_dir(f.parent().unwrap()).unwrap().count();
        assert_eq!(count, 1);
    }
}
