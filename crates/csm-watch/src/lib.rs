//! Shared event sources: turn a transport into a stream of [`csm_core::Event`]s.
//!
//! Reused by both the desktop app and the remote `csm-agent` so the rollout /
//! file-bus watching logic lives in one place.

use std::sync::mpsc::Sender;

use csm_core::Event;

pub mod codex_rollout;
pub mod discover;
pub mod fs_watch;
pub mod ntfy;

pub use codex_rollout::CodexRolloutSource;
pub use discover::discover_sessions;
pub use fs_watch::FsWatchSource;
pub use ntfy::NtfySource;

/// Produces events into `tx`. Implementations typically block, so run on a
/// dedicated thread.
pub trait Source {
    fn run(self, tx: Sender<Event>);
}
