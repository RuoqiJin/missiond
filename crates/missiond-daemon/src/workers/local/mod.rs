//! Local workers — pure computation, no LLM dependency.
//! Workers in this directory get no ControlTree provider dependencies.

pub(crate) mod ast_sync_worker;
pub(crate) mod code_prefetch;
pub(crate) mod conversation_logger;
pub(crate) mod event_analyzer_worker;
pub(crate) mod conversation_organizer;
pub(crate) mod experience_harvester;
pub(crate) mod gemini_logger;
pub(crate) mod gemini_reconcile_worker;
pub(crate) mod pty_event_worker;
pub(crate) mod reconcile_worker;
pub(crate) mod tagger_chunker;

// Re-exports so workers can use `super::BackgroundWorker` etc.
pub(crate) use super::{BackgroundWorker, WorkerContext, WorkerKind};
