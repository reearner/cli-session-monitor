//! Shared contract between `session-reporter` and the desktop app.
//!
//! The unified [`Event`] schema is the *only* cross-process contract; it carries
//! a [`SCHEMA_VERSION`] so it can evolve, and a `host` field so the same types
//! work unchanged for local and (Phase 2) remote / cross-device sessions.

mod event;
pub mod installer;
pub mod paths;
pub mod pathmatch;

pub use event::{Event, EventKind, SessionKey, Source, SCHEMA_VERSION};
pub use paths::host_name;
