//! PTY Module - Terminal session management for Claude Code
//!
//! Architecture: portable-pty (process) + alacritty_terminal (emulation) + semantic (detection)
//!
//! # Components
//! - `PTYSession`: Single interactive Claude Code session
//! - `PTYManager`: Manages multiple PTY sessions
//! - `IncrementalExtractor`: Frame-by-frame text extraction

pub mod anomaly;
mod extractor;
mod manager;
mod pty_recognition;
pub mod screenshot;
mod session;

pub use extractor::{
    FrameDelta, IncrementalExtractor, LineData, ScreenSnapshot, StableTextOp, TextAssembler,
};
pub use manager::{
    ManagerEvent, ManagerStats, PTYAgentInfo, PTYExecuteResult, PTYManager, PTYSpawnOptions,
    PermissionPolicy, Slot,
};
pub use pty_recognition::{
    recognize_screen, session_state_snapshot, PtyCanonicalState, PtyRecognitionSnapshot,
};
pub use session::{
    ConfirmInfo, ConfirmResponse, McpReconnectOutcome, Message, MessageRole, PTYSession,
    PTYSessionOptions, PermissionDecision, ScreenTextEvent, ScreenTextSource, SessionEvent,
    SessionState, TextOutputEvent, ToolInfo,
};
