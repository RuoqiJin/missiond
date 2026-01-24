//! Semantic terminal parsing module
//!
//! This module provides parsers for detecting terminal states and parsing
//! confirmation dialogs from Claude Code CLI output.

mod confirm;
mod state;
mod types;

pub use confirm::ClaudeCodeConfirmParser;
pub use state::ClaudeCodeStateParser;
pub use types::*;
