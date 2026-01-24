//! Claude Code Tasks Integration
//!
//! Monitors Claude Code sessions and their tasks in real-time.
//! Reads from ~/.claude/projects/{path}/{session}.jsonl
//!
//! # Components
//! - `CCTasksWatcher`: Watches for file changes and emits events
//! - `parser`: Parses sessions-index.json and JSONL files
//! - `types`: Data structures for sessions, tasks, and events

mod parser;
mod types;
mod watcher;

pub use parser::*;
pub use types::*;
pub use watcher::{CCTasksWatcher, CCTasksWatcherOptions, WatcherEvent};
