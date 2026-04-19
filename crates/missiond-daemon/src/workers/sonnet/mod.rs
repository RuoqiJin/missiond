//! Sonnet API workers — depend on SonnetGateway.
//! All workers in this directory auto-receive CtlProvider::Sonnet dependency.

pub(crate) mod arch_maintenance_worker;
// v1.3.0 SSOT cutover: briefing_worker removed (UPDATE semantics incompatible
// with append-only event_log, see intent-event-bus-execution.lisp D015).
pub(crate) mod embedding_worker;
pub(crate) mod lisp_survey_worker;
pub(crate) mod retro_worker;
pub(crate) mod translation_worker;

// Re-exports so workers can use `super::BackgroundWorker` etc.
pub(crate) use super::{BackgroundWorker, WorkerContext, WorkerKind};
