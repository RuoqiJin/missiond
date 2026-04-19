//! Storage primitives for [`super::InMemoryLog`] — the internal row type
//! plus capacity constant shared between the log and its writer task.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 testing-story
//! in-memory-breakdown: `storage.rs` hosts the plain data definitions so
//! `log.rs` can stay focused on the `Log` trait surface and
//! `writer_task.rs` on the single-writer serialization loop.

use uuid::Uuid;

use super::super::domain::Domain;
use super::super::log::Seq;

/// Bound on the in-memory append channel. Matches the PG default
/// (frozen lisp §4.2.b) so behavioral parity holds.
pub const IN_MEMORY_APPEND_CAPACITY: usize = 4096;

/// Internal stored row. Kept in the Vec verbatim so `read_from` / `head_seq`
/// can replay.
#[derive(Debug, Clone)]
pub struct StoredRow {
    pub seq: Seq,
    pub domain: Domain,
    pub kind: String,
    pub payload_inline: Option<serde_json::Value>,
    pub payload_ref: Option<String>,
    pub producer_id: String,
    pub dedupe_key: Option<Uuid>,
    pub causation_depth: i16,
    pub trace_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub ephemeral: bool,
}
