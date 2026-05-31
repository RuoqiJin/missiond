//! Antigravity (`agy`) CLI conversation ingestion.
//!
//! Antigravity writes a per-session JSONL trajectory at
//! `~/.gemini/antigravity-cli/brain/<conversationId>/.system_generated/logs/transcript_full.jsonl`
//! and an input index at `~/.gemini/antigravity-cli/history.jsonl` carrying
//! `{conversationId, workspace, display, timestamp}`.
//!
//! This module mirrors `gemini_cli`: the parser flattens Antigravity steps into
//! the shared `CCMessageLine` shape, and `AgyCliWatcher` emits
//! `WatcherEvent::NewMessages { source: "agy_cli" }` into the same broadcast
//! channel consumed by ConversationLoggerWorker. There is no bespoke completion
//! path — agy turns land in the durable conversation store like every other
//! provider, and consumers (autopilot completion, mission_conversation_query)
//! read them on demand.

mod parser;
mod types;
mod watcher;

pub use parser::*;
pub use types::*;
pub use watcher::{AgyCliWatcher, AgyCliWatcherOptions};
