//! Sonnet API workers — depend on SonnetGateway.
//! All workers in this directory auto-receive CtlProvider::Sonnet dependency.

pub(crate) mod arch_maintenance_worker;
pub(crate) mod briefing_worker;
pub(crate) mod embedding_worker;
pub(crate) mod lisp_survey_worker;
pub(crate) mod retro_worker;
pub(crate) mod translation_worker;

// Re-exports so workers can use `super::BackgroundWorker` etc.
pub(crate) use super::{BackgroundWorker, WorkerContext, WorkerKind};
