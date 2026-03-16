//! Semantic terminal parsing module
//!
//! This module provides parsers for detecting terminal states and parsing
//! confirmation dialogs from Claude Code CLI output.

mod confirm;
pub mod fingerprint;
mod gemini_state;
pub mod patterns;
mod state;
mod status;
mod title;
mod tool;
mod types;

pub use confirm::ClaudeCodeConfirmParser;
pub use fingerprint::{
    claude_code_fingerprints, claude_code_fingerprints_from, default_registry, registry_from,
    Fingerprint, FingerprintCategory, FingerprintHints, FingerprintMatch, FingerprintPattern,
    FingerprintRegistry, FingerprintResult, FingerprintType, CLAUDE_CODE_FINGERPRINTS,
};
pub use gemini_state::GeminiCliStateParser;
pub use state::ClaudeCodeStateParser;
pub use status::{ClaudeCodeStatusParser, SPINNER_CHARS};
pub use title::{ClaudeCodeTitleParser, ALL_SPINNERS, BRAILLE_SPINNERS, OTHER_SPINNERS};
pub use patterns::{
    default_compiled, global_patterns, maybe_reload_global_patterns, CompiledPatterns,
    PatternConfig,
};
pub use tool::{ClaudeCodeToolOutputParser, KNOWN_TOOLS};
pub use types::*;
