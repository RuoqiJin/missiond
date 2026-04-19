//! missiond-core - Core library for Mission Control daemon
//!
//! This crate provides core functionality for missiond, including:
//! - Core management (SlotManager, PermissionPolicy, MissionControl)
//! - Core types (Task, InboxMessage, TaskEvent, etc.)
//! - Semantic terminal parsing (state detection, confirmation dialogs)
//! - PTY session management
//! - Database operations
//! - WebSocket communication
//! - Claude Code Tasks monitoring

pub mod ast;
pub mod cc_tasks;
pub mod core;
pub mod gemini_cli;
pub mod db;
pub mod embedding;
pub mod event;
pub mod ipc;
pub use missiond_pty as pty;
pub use semantic_terminal as semantic;
pub mod semantic_parsing;
pub mod skill;
pub mod sync;
pub mod types;
pub mod util;
pub mod ws;

// Re-export core management types
pub use crate::core::{
    ExecutionMode, Inbox, LearnedPermission, LearnedPermissions, MissionControl,
    MissionControlOptions, PermissionConfig, PermissionDecision as CorePermissionDecision,
    PermissionPolicy, PermissionRule, SlotManager, SlotReloadResult,
};

// Re-export core types
pub use types::{
    CliEngine, CreateTaskInput, Credential, EventType, InboxMessage, InfraConfig, InfraServer,
    KBOperation, KBOperationRow, KBRememberInput, KnowledgeEntry, Lifecycle, SkillBlock, SkillSearchResult,
    SkillExecutionStat, SkillTopic, SkillVersion, Slot, SlotConfig, SlotsConfig, Task, TaskEvent, TaskStatus, TaskUpdate,
};

// Re-export semantic parser types
pub use semantic::{
    ClaudeCodeConfirmParser, ClaudeCodeStateParser, ConfirmAction, ConfirmOption,
    ConfirmParser, ConfirmType, ParserContext, ParserMeta, State,
    StateDetectionResult, StateParser,
};

// Re-export PTY types
pub use pty::{
    ConfirmInfo, ConfirmResponse, FrameDelta, IncrementalExtractor, LineData, ManagerEvent,
    ManagerStats, Message, MessageRole, PTYAgentInfo, PTYExecuteResult, PTYManager, PTYSession,
    PTYSessionOptions, PTYSpawnOptions, PermissionDecision, PermissionPolicy as PTYPermissionPolicy,
    ScreenSnapshot, ScreenTextEvent, ScreenTextSource, SessionEvent, SessionState, Slot as PTYSlot,
    StableTextOp, TextAssembler, TextOutputEvent, ToolInfo,
};

// Re-export CC Tasks types
pub use cc_tasks::{
    extract_last_assistant_text, jsonl_has_completed_turn, CCInProgressTask, CCMessage,
    CCMessageLine, CCSession, CCSessionIndex, CCSessionIndexEntry, CCTask, CCTaskChangeEvent,
    CCTaskStatus, CCTasksOverview, CCTasksWatcher, CCTasksWatcherOptions, TasksByStatus,
    WatcherEvent,
};

// Re-export Gemini CLI types
pub use gemini_cli::{GeminiCliWatcher, GeminiCliWatcherOptions};

// Re-export WebSocket types
pub use ws::{PTYWebSocketServer, WSServerOptions, ContextEnricherFn, ContextEnricherSlot, ContextEnrichResult};

// Re-export Sync types
pub use sync::{SyncClient, SyncClientOptions, SyncEvent, SyncRelay, SyncRelayOptions};

// Re-export IPC types
pub use ipc::{
    default_ipc_endpoint, default_mission_home, ipc_endpoint, IpcListener, IpcStream,
};

// Re-export Skill types
pub use skill::{
    ContextHook, SkillAction, SkillIndex, SkillMeta, SkillRequires, WorkflowBlock, WorkflowResult,
    WorkflowStep, WorkflowStepPreview, WorkflowStepResult, parse_workflow_blocks, resolve_vars,
};
pub mod services;
