//! Output destination for normalized events.
//!
//! MVP ships [`FileSink`] (local file bus). The trait is deliberately small so a
//! Phase 2 `NetworkSink` (push to a relay endpoint with auth + minimal metadata)
//! can be added without touching adapters or the state machine.

use csm_core::Event;

pub mod file;

pub use file::FileSink;

/// A place to deliver a normalized [`Event`].
pub trait Sink {
    fn emit(&self, event: &Event) -> Result<(), Box<dyn std::error::Error>>;
}
