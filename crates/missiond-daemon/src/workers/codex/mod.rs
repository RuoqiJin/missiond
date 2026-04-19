//! Codex CLI workers — depend on Claude Code / Codex CLI.
//! All workers in this directory auto-receive CtlProvider::Codex dependency.

// step_narrator removed v0.4.23 Phase 6 (message_narrations + narration_cursors dropped).
pub(crate) mod vision_worker;

// Re-exports so workers can use `super::BackgroundWorker` etc.
pub(crate) use super::{BackgroundWorker, WorkerContext, WorkerKind};
