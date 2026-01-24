//! PTY Module - Terminal session management for Claude Code
//!
//! Architecture: portable-pty (process) + alacritty_terminal (emulation) + semantic (detection)
//!
//! # Components
//! - `PTYSession`: Single interactive Claude Code session
//! - `PTYManager`: Manages multiple PTY sessions
//! - `IncrementalExtractor`: Frame-by-frame text extraction

mod extractor;
mod manager;
mod session;

pub use extractor::{
    FrameDelta, IncrementalExtractor, LineData, ScreenSnapshot, StableTextOp, TextAssembler,
};
pub use manager::{
    ManagerEvent, ManagerStats, PTYAgentInfo, PTYExecuteResult, PTYManager, PTYSpawnOptions,
    PermissionPolicy, Slot,
};
pub use session::{
    ConfirmInfo, ConfirmResponse, Message, MessageRole, PTYSession, PTYSessionOptions,
    PermissionDecision, ScreenTextEvent, ScreenTextSource, SessionEvent, SessionState,
    TextOutputEvent, ToolInfo,
};
