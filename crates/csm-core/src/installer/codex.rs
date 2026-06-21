//! Codex integration: set `notify` in `~/.codex/config.toml` to call
//! `session-reporter --source codex`.
//!
//! Codex allows only a single `notify` program, so this installer is careful:
//! - no `notify`        -> set ours
//! - our `notify`       -> idempotent (refresh path only if the app moved)
//! - someone else's     -> **conflict**: never overwritten; reported to the user
//!
//! `toml_edit` is used so the user's comments and formatting survive untouched.

use std::io;
use std::path::Path;

use toml_edit::{value, Array, DocumentMut, Item};

use super::{backup_file, write_atomic, InstallOutcome};

const MARKER: &str = "session-reporter";

fn read_doc(path: &Path) -> io::Result<DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(DocumentMut::new()),
        Ok(s) => s.parse::<DocumentMut>().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("config.toml parse error: {e}"),
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(e) => Err(e),
    }
}

fn desired(reporter: &Path) -> Array {
    let mut a = Array::new();
    a.push(reporter.display().to_string());
    a.push("--source");
    a.push("codex");
    a
}

fn item_strings(item: &Item) -> Vec<String> {
    item.as_array()
        .map(|a| {
            a.iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Is this `notify` ours? (contains our reporter executable)
fn is_ours(item: &Item) -> bool {
    item_strings(item).iter().any(|s| s.contains(MARKER))
}

fn matches_desired(item: &Item, reporter: &Path) -> bool {
    let want: Vec<String> = desired(reporter)
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect();
    item_strings(item) == want
}

fn render(item: &Item) -> String {
    item.to_string().trim().to_string()
}

pub fn status(config_path: &Path) -> io::Result<InstallOutcome> {
    let doc = read_doc(config_path)?;
    let mut out = InstallOutcome::new(config_path);
    match doc.get("notify") {
        Some(it) if is_ours(it) => {
            out.installed = true;
            out.summary = "Codex integration installed".to_string();
        }
        Some(it) => {
            out.conflict = Some(render(it));
            out.summary = format!(
                "Codex `notify` is already in use ({}) — not installed",
                render(it)
            );
        }
        None => out.summary = "Codex integration not installed".to_string(),
    }
    Ok(out)
}

pub fn install(config_path: &Path, reporter: &Path) -> io::Result<InstallOutcome> {
    let mut doc = read_doc(config_path)?;
    let mut out = InstallOutcome::new(config_path);

    match doc.get("notify") {
        // Foreign notify — never overwrite.
        Some(it) if !is_ours(it) => {
            out.conflict = Some(render(it));
            out.summary = format!(
                "Found an existing Codex `notify` ({}). Left unchanged so your config isn't broken; resolve that notify manually first if you want to integrate.",
                render(it)
            );
            return Ok(out);
        }
        // Already ours and up to date.
        Some(it) if matches_desired(it, reporter) => {
            out.installed = true;
            out.summary =
                "Codex integration already present — nothing to do (idempotent)".to_string();
            return Ok(out);
        }
        _ => {}
    }

    // Either no notify, or ours but stale (app moved) -> (re)write.
    out.backup_path = backup_file(config_path)?;
    doc["notify"] = value(desired(reporter));
    write_atomic(config_path, &doc.to_string())?;
    out.changed = true;
    out.installed = true;
    out.summary = format!(
        "Set notify = [\"{}\", \"--source\", \"codex\"] in {}. {}",
        reporter.display(),
        config_path.display(),
        out.backup_path
            .as_ref()
            .map(|b| format!("Original backed up to {}", b.display()))
            .unwrap_or_else(|| "(no existing file — created)".to_string())
    );
    Ok(out)
}

pub fn uninstall(config_path: &Path) -> io::Result<InstallOutcome> {
    let mut out = InstallOutcome::new(config_path);
    if !config_path.exists() {
        out.summary = "config.toml does not exist — nothing to uninstall".to_string();
        return Ok(out);
    }
    let mut doc = read_doc(config_path)?;
    match doc.get("notify") {
        Some(it) if is_ours(it) => {
            out.backup_path = backup_file(config_path)?;
            doc.remove("notify");
            write_atomic(config_path, &doc.to_string())?;
            out.changed = true;
            out.summary = format!(
                "Removed our notify from {}. {}",
                config_path.display(),
                out.backup_path
                    .as_ref()
                    .map(|b| format!("Original backed up to {}", b.display()))
                    .unwrap_or_default()
            );
        }
        Some(it) => {
            out.conflict = Some(render(it));
            out.summary = "The existing `notify` isn't ours — left unchanged".to_string();
        }
        None => out.summary = "No notify found — no changes".to_string(),
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
    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn install_into_missing_sets_notify() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let out = install(&cfg, &reporter()).unwrap();
        assert!(out.changed && out.installed && out.conflict.is_none());
        let doc = read(&cfg).parse::<DocumentMut>().unwrap();
        assert!(is_ours(doc.get("notify").unwrap()));
    }

    #[test]
    fn install_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        install(&cfg, &reporter()).unwrap();
        let second = install(&cfg, &reporter()).unwrap();
        assert!(!second.changed && second.installed);
    }

    #[test]
    fn install_preserves_existing_config_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(
            &cfg,
            "# my codex config\nmodel = \"gpt-5.5\"\n\n[projects.'d:\\\\x']\ntrust_level = \"trusted\"\n",
        )
        .unwrap();
        install(&cfg, &reporter()).unwrap();
        let text = read(&cfg);
        assert!(text.contains("# my codex config"), "comment preserved");
        assert!(text.contains("model = \"gpt-5.5\""), "settings preserved");
        assert!(text.contains("trust_level"), "section preserved");
        assert!(text.contains("notify"), "our notify added");
    }

    #[test]
    fn foreign_notify_is_not_overwritten_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "notify = [\"/usr/bin/other-notifier\"]\n").unwrap();
        let out = install(&cfg, &reporter()).unwrap();
        assert!(!out.changed, "must not modify a foreign notify");
        assert!(out.conflict.is_some());
        assert!(
            read(&cfg).contains("other-notifier"),
            "foreign config intact"
        );
    }

    #[test]
    fn uninstall_removes_only_ours() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "model = \"gpt-5.5\"\n").unwrap();
        install(&cfg, &reporter()).unwrap();
        let out = uninstall(&cfg).unwrap();
        assert!(out.changed);
        let text = read(&cfg);
        assert!(!text.contains("notify"), "our notify removed");
        assert!(
            text.contains("model = \"gpt-5.5\""),
            "user config preserved"
        );
    }

    #[test]
    fn uninstall_leaves_foreign_notify() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "notify = [\"/usr/bin/other-notifier\"]\n").unwrap();
        let out = uninstall(&cfg).unwrap();
        assert!(!out.changed);
        assert!(read(&cfg).contains("other-notifier"));
    }

    #[test]
    fn status_detects_states() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        assert!(!status(&cfg).unwrap().installed);
        install(&cfg, &reporter()).unwrap();
        assert!(status(&cfg).unwrap().installed);
    }

    #[test]
    fn parse_error_aborts_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "this = = = not toml").unwrap();
        assert!(install(&cfg, &reporter()).is_err());
        assert_eq!(read(&cfg), "this = = = not toml");
    }
}
