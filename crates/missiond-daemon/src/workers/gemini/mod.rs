//! Gemini CLI workers — depend on Gemini CLI (via SlotManager PTY).
//! All workers in this directory auto-receive CtlProvider::Gemini dependency.

pub(crate) mod strategy_worker;

// Re-exports so workers can use `super::BackgroundWorker` etc.
pub(crate) use super::{BackgroundWorker, WorkerContext, WorkerKind};
