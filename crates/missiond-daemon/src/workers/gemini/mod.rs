//! Gemini CLI workers — depend on Gemini CLI (via SlotManager PTY).
//! All workers in this directory auto-receive CtlProvider::Gemini dependency.

pub(crate) mod strategy_worker;

// No re-exports needed — strategy_worker is event-driven, not BackgroundWorker.
