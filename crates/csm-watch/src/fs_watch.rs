//! Watches `~/.cli-session-monitor/events/` and converts each event file into an
//! [`Event`] on the channel.
//!
//! On start it **drains** any files already present (so the UI recovers after a
//! restart), then watches for new ones. Files are **consumed** (deleted) after
//! reading, which keeps the directory small and naturally de-duplicates the
//! Create/Modify events a single atomic rename can produce. Unparseable files
//! are skipped (and removed) rather than crashing the watcher.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use csm_core::Event;
use notify::{RecursiveMode, Watcher};

use crate::Source;

pub struct FsWatchSource {
    dir: PathBuf,
}

impl FsWatchSource {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    /// `~/.cli-session-monitor/events` (via shared `csm_core::paths`).
    pub fn default_dir() -> PathBuf {
        csm_core::paths::events_dir()
    }
}

impl Source for FsWatchSource {
    fn run(self, tx: Sender<Event>) {
        let dir = self.dir;
        let _ = std::fs::create_dir_all(&dir);

        // 1) Recover current view from whatever is already on disk.
        drain(&dir, &tx);

        // 2) Watch for new files (non-recursive so the `.tmp` staging dir, and
        //    in-progress writes, are never observed).
        let (raw_tx, raw_rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("fs-watch: failed to create watcher: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
            eprintln!("fs-watch: failed to watch {}: {e}", dir.display());
            return;
        }

        for res in raw_rx {
            match res {
                Ok(event) if is_relevant(&event.kind) => {
                    for path in event.paths {
                        if is_event_file(&path) {
                            process_file(&path, &tx);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("fs-watch: event error: {e}"),
            }
        }
    }
}

fn is_relevant(kind: &notify::EventKind) -> bool {
    use notify::EventKind::{Create, Modify};
    matches!(kind, Create(_) | Modify(_))
}

fn is_event_file(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|e| e == "json")
}

/// Extract the leading millisecond timestamp from `"<ts>_<uuid>.json"`.
fn ts_from_name(path: &Path) -> i64 {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.split('_').next())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

/// Read + parse one file; `None` (with a log) on any problem. Never panics.
fn read_event_file(path: &Path) -> Option<Event> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Event>(&text) {
        Ok(ev) => Some(ev),
        Err(e) => {
            eprintln!("fs-watch: skipping unparseable {}: {e}", path.display());
            None
        }
    }
}

/// Read an event file, forward it if valid, then delete it (consume). Deleting
/// also clears junk/corrupt files so they aren't retried every startup.
fn process_file(path: &Path, tx: &Sender<Event>) {
    if let Some(ev) = read_event_file(path) {
        let _ = tx.send(ev);
    }
    let _ = std::fs::remove_file(path);
}

/// Read all existing event files in chronological order, forwarding and consuming
/// each. Missing directory is a no-op.
pub(crate) fn drain(dir: &Path, tx: &Sender<Event>) {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| is_event_file(p))
            .collect(),
        Err(_) => return,
    };
    files.sort_by_key(|p| ts_from_name(p));
    for f in files {
        process_file(&f, tx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use csm_core::{EventKind, Source as Src};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    fn put(dir: &Path, ev: &Event, n: u32) {
        let name = format!("{}_{}.json", ev.ts, n);
        std::fs::write(dir.join(name), serde_json::to_string(ev).unwrap()).unwrap();
    }

    fn sample(ts: i64, kind: EventKind) -> Event {
        Event::new(Src::ClaudeCode, "s", "/p", "h", kind, ts)
    }

    #[test]
    fn drain_reads_sorted_and_consumes() {
        let dir = tempfile::tempdir().unwrap();
        // Write out of order; drain must emit ascending by ts.
        put(dir.path(), &sample(2000, EventKind::RunEnd), 2);
        put(dir.path(), &sample(1000, EventKind::RunStart), 1);

        let (tx, rx) = channel();
        drain(dir.path(), &tx);

        assert_eq!(rx.recv().unwrap().ts, 1000);
        assert_eq!(rx.recv().unwrap().ts, 2000);
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "files are consumed"
        );
    }

    #[test]
    fn drain_skips_corrupt_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1_1.json"), "{ not json").unwrap();
        let (tx, rx) = channel();
        drain(dir.path(), &tx);
        assert!(rx.try_recv().is_err(), "nothing forwarded");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "junk removed so it isn't retried"
        );
    }

    #[test]
    fn drain_on_missing_dir_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = channel();
        drain(&dir.path().join("does-not-exist"), &tx); // must not panic
    }

    #[test]
    fn ignores_non_json_and_tmp_subdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".tmp")).unwrap();
        std::fs::write(dir.path().join("note.txt"), "hello").unwrap();
        let (tx, rx) = channel();
        drain(dir.path(), &tx);
        assert!(rx.try_recv().is_err());
    }

    // Live watcher wiring is timing-dependent; ignored by default so `cargo test`
    // stays deterministic in CI. Run manually with `cargo test -- --ignored`.
    #[test]
    #[ignore = "timing-dependent fs watcher; run locally with --ignored"]
    fn watcher_picks_up_new_files() {
        // Leak the dir (keep) so it isn't removed while the watcher holds it.
        let dir = tempfile::tempdir().unwrap().keep();
        let (tx, rx) = channel();
        let src = FsWatchSource::new(&dir);
        std::thread::spawn(move || src.run(tx));
        std::thread::sleep(Duration::from_millis(400)); // let the watcher arm

        // Emulate FileSink: write into .tmp then atomically rename into place.
        let ev = sample(1234, EventKind::RunEnd);
        let tmp = dir.join(".tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let tmp_file = tmp.join("1234_x.json");
        std::fs::write(&tmp_file, serde_json::to_string(&ev).unwrap()).unwrap();
        std::fs::rename(&tmp_file, dir.join("1234_x.json")).unwrap();

        let got = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("event delivered within 3s");
        assert_eq!(got.ts, 1234);
    }
}
