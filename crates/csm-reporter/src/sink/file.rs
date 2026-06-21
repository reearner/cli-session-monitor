use std::fs;
use std::path::{Path, PathBuf};

use csm_core::Event;

use super::Sink;

/// Writes each event as a JSON file using **temp-file + atomic rename** so a
/// watcher on the events directory never observes a half-written file.
///
/// Layout:
/// ```text
/// <events_dir>/.tmp/<ts>_<uuid>.json   (written here first)
/// <events_dir>/<ts>_<uuid>.json        (atomically renamed into place)
/// ```
/// Rename within the same filesystem is atomic, and the `.tmp` subdirectory keeps
/// in-progress writes out of the directory the watcher scans.
pub struct FileSink {
    events_dir: PathBuf,
    tmp_dir: PathBuf,
}

impl FileSink {
    /// Create a sink rooted at `events_dir`, creating it (and `.tmp`) if missing.
    pub fn new(events_dir: impl AsRef<Path>) -> std::io::Result<Self> {
        let events_dir = events_dir.as_ref().to_path_buf();
        let tmp_dir = events_dir.join(".tmp");
        fs::create_dir_all(&tmp_dir)?;
        Ok(Self {
            events_dir,
            tmp_dir,
        })
    }

    fn file_name(event: &Event) -> String {
        // `ts` prefix gives rough chronological ordering for the startup drain;
        // the uuid guarantees uniqueness even within the same millisecond.
        format!("{}_{}.json", event.ts, uuid::Uuid::new_v4())
    }
}

impl Sink for FileSink {
    fn emit(&self, event: &Event) -> Result<(), Box<dyn std::error::Error>> {
        let name = Self::file_name(event);
        let tmp_path = self.tmp_dir.join(&name);
        let final_path = self.events_dir.join(&name);

        let json = serde_json::to_vec(event)?;
        fs::write(&tmp_path, &json)?;
        fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use csm_core::{EventKind, Source};

    fn sample(ts: i64) -> Event {
        Event::new(
            Source::ClaudeCode,
            "s1",
            "/proj",
            "host",
            EventKind::RunStart,
            ts,
        )
    }

    #[test]
    fn writes_parseable_event_file() {
        let dir = tempfile::tempdir().unwrap();
        let sink = FileSink::new(dir.path()).unwrap();
        let ev = sample(123);
        sink.emit(&ev).unwrap();

        let mut files: Vec<PathBuf> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        assert_eq!(files.len(), 1);
        let content = fs::read_to_string(files.pop().unwrap()).unwrap();
        let back: Event = serde_json::from_str(&content).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn creates_dir_if_missing() {
        let base = tempfile::tempdir().unwrap();
        let nested = base.path().join("a").join("b").join("events");
        let sink = FileSink::new(&nested).unwrap();
        sink.emit(&sample(1)).unwrap();
        assert!(nested.exists());
    }

    #[test]
    fn leaves_no_tmp_residue() {
        let dir = tempfile::tempdir().unwrap();
        let sink = FileSink::new(dir.path()).unwrap();
        sink.emit(&sample(1)).unwrap();
        let tmp_count = fs::read_dir(dir.path().join(".tmp")).unwrap().count();
        assert_eq!(tmp_count, 0);
    }

    #[test]
    fn distinct_events_get_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        let sink = FileSink::new(dir.path()).unwrap();
        sink.emit(&sample(1)).unwrap();
        sink.emit(&sample(1)).unwrap();
        let count = fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "json")
            })
            .count();
        assert_eq!(count, 2);
    }
}
