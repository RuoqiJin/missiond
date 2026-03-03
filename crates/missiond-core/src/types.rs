//! Core types for missiond
//!
//! Mirrors the TypeScript definitions in packages/missiond/src/types.ts

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ============ Slot Config ============

/// Slot traits: declarative capabilities that control pipeline routing.
/// Used to determine which slots' conversations enter extraction pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotTrait {
    /// Slot is a system/meta agent (memory extraction, code review, GC, etc.).
    /// Its conversations are excluded from all extraction pipelines.
    IsMetaAgent,
    /// Slot produces conversations that should be analyzed for knowledge extraction.
    GeneratesKnowledge,
}

/// Configuration for a slot (workstation)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotConfig {
    pub id: String,
    pub role: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_start: Option<bool>,
    /// Skip all permission prompts and trust dialogs (--dangerously-skip-permissions)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dangerously_skip_permissions: Option<bool>,
    /// Declarative traits controlling pipeline behavior.
    /// If empty/absent, defaults are inferred from role at load time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<SlotTrait>,
    /// Custom environment variables injected into the PTY child process.
    /// Supports `${secret:path}` syntax for Secret Store resolution.
    /// Used for per-slot model provider configuration (e.g., MiniMax M2.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

impl SlotConfig {
    /// Is this a system/meta agent whose conversations should be excluded from pipelines?
    pub fn is_meta_agent(&self) -> bool {
        self.traits.contains(&SlotTrait::IsMetaAgent)
    }

    /// Apply default traits based on role if none were explicitly configured.
    pub fn apply_default_traits(&mut self) {
        if !self.traits.is_empty() {
            return;
        }
        match self.role.as_str() {
            "memory" => self.traits.push(SlotTrait::IsMetaAgent),
            _ => {}
        }
    }
}

/// Slot = Config + session (process state managed by ProcessManager)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    #[serde(flatten)]
    pub config: SlotConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

// ============ Task ============

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(TaskStatus::Queued),
            "running" => Some(TaskStatus::Running),
            "done" => Some(TaskStatus::Done),
            "failed" => Some(TaskStatus::Failed),
            "cancelled" => Some(TaskStatus::Cancelled),
            _ => None,
        }
    }
}

/// A task to be executed
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub role: String,
    pub prompt: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}

/// Input for creating a new task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskInput {
    pub role: String,
    pub prompt: String,
}

// ============ Inbox ============

/// A message in the inbox
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxMessage {
    pub id: String,
    pub task_id: String,
    pub from_role: String,
    pub content: String,
    pub read: bool,
    pub created_at: i64,
}

// ============ Event ============

/// Event type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    TaskCreated,
    TaskStarted,
    TaskProgress,
    TaskDone,
    TaskFailed,
    TaskCancelled,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::TaskCreated => "task_created",
            EventType::TaskStarted => "task_started",
            EventType::TaskProgress => "task_progress",
            EventType::TaskDone => "task_done",
            EventType::TaskFailed => "task_failed",
            EventType::TaskCancelled => "task_cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "task_created" => Some(EventType::TaskCreated),
            "task_started" => Some(EventType::TaskStarted),
            "task_progress" => Some(EventType::TaskProgress),
            "task_done" => Some(EventType::TaskDone),
            "task_failed" => Some(EventType::TaskFailed),
            "task_cancelled" => Some(EventType::TaskCancelled),
            _ => None,
        }
    }
}

/// A task event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub id: i64,
    pub task_id: String,
    #[serde(rename = "type")]
    pub event_type: EventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    pub timestamp: i64,
}

// ============ Config ============

/// Slots configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotsConfig {
    pub slots: Vec<SlotConfig>,
}

// ============ Task Update ============

/// Partial update for a task
#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub status: Option<TaskStatus>,
    pub slot_id: Option<String>,
    pub session_id: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

// ============ Board Task (Personal Task Board) ============

/// Board task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardTaskStatus {
    Open,
    Running,
    Verifying,
    Done,
    Blocked,
    Failed,
    Skipped,
}

impl BoardTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BoardTaskStatus::Open => "open",
            BoardTaskStatus::Running => "running",
            BoardTaskStatus::Verifying => "verifying",
            BoardTaskStatus::Done => "done",
            BoardTaskStatus::Blocked => "blocked",
            BoardTaskStatus::Failed => "failed",
            BoardTaskStatus::Skipped => "skipped",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "open" => Some(BoardTaskStatus::Open),
            "running" => Some(BoardTaskStatus::Running),
            "verifying" => Some(BoardTaskStatus::Verifying),
            "done" => Some(BoardTaskStatus::Done),
            "blocked" => Some(BoardTaskStatus::Blocked),
            "failed" => Some(BoardTaskStatus::Failed),
            "skipped" => Some(BoardTaskStatus::Skipped),
            _ => None,
        }
    }
}

// ============ Engineering Flow ============

/// Phase in the engineering task flow state machine.
/// Daemon drives transitions; ConsultGemini phases are daemon-only (no PTY).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineeringPhase {
    /// Slot investigates code and existing implementation
    Investigate,
    /// Daemon calls Gemini with investigation report (no PTY)
    ConsultGemini1,
    /// Slot writes execution plan based on Gemini advice
    Plan,
    /// Daemon sends plan to Gemini for review (no PTY)
    ConsultGemini2,
    /// Slot implements the plan
    Execute,
    /// Slot commits and updates skill docs
    Finalize,
    /// Flow complete
    Done,
}

impl EngineeringPhase {
    pub fn next(&self) -> Option<Self> {
        match self {
            Self::Investigate => Some(Self::ConsultGemini1),
            Self::ConsultGemini1 => Some(Self::Plan),
            Self::Plan => Some(Self::ConsultGemini2),
            Self::ConsultGemini2 => Some(Self::Execute),
            Self::Execute => Some(Self::Finalize),
            Self::Finalize => Some(Self::Done),
            Self::Done => None,
        }
    }

    /// Whether this phase is executed by daemon (no PTY involvement)
    pub fn is_daemon_phase(&self) -> bool {
        matches!(self, Self::ConsultGemini1 | Self::ConsultGemini2)
    }

    /// Whether this phase requires a slot/PTY
    pub fn is_slot_phase(&self) -> bool {
        !self.is_daemon_phase() && *self != Self::Done
    }

    /// Timeout in seconds for this phase
    pub fn timeout_secs(&self) -> u64 {
        match self {
            Self::Investigate | Self::Plan => 900,        // 15min
            Self::ConsultGemini1 | Self::ConsultGemini2 => 120, // 2min
            Self::Execute => 3600,                         // 60min (复杂任务需要更多时间)
            Self::Finalize => 600,                         // 10min
            Self::Done => 0,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Investigate => "Investigate",
            Self::ConsultGemini1 => "ConsultGemini1",
            Self::Plan => "Plan",
            Self::ConsultGemini2 => "ConsultGemini2",
            Self::Execute => "Execute",
            Self::Finalize => "Finalize",
            Self::Done => "Done",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "investigate" => Some(Self::Investigate),
            "consult_gemini1" | "consult_gemini_1" => Some(Self::ConsultGemini1),
            "plan" => Some(Self::Plan),
            "consult_gemini2" | "consult_gemini_2" => Some(Self::ConsultGemini2),
            "execute" => Some(Self::Execute),
            "finalize" => Some(Self::Finalize),
            "done" => Some(Self::Done),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Investigate => "investigate",
            Self::ConsultGemini1 => "consult_gemini1",
            Self::Plan => "plan",
            Self::ConsultGemini2 => "consult_gemini2",
            Self::Execute => "execute",
            Self::Finalize => "finalize",
            Self::Done => "done",
        }
    }
}

/// Context passed between flow phases (serialized as JSON in DB)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub investigation_report: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini_advice_1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini_advice_2: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
}

/// A personal task on the board
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTask {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: BoardTaskStatus,
    pub priority: String,   // high, medium, low
    pub category: String,   // deploy, dev, infra, test, other
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Assigned PTY slot ID (e.g., "slot-coder-1") for autopilot execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    /// Whether to auto-execute when due_date is reached
    #[serde(default)]
    pub auto_execute: bool,
    /// Prompt template for autopilot (overrides title+description)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_template: Option<String>,
    /// Hidden from default list view (e.g. renewal/manual tasks)
    #[serde(default)]
    pub hidden: bool,
    /// Retry count for autopilot execution failures
    #[serde(default)]
    pub retry_count: i64,
    /// Max retries before marking as failed (default 2)
    #[serde(default = "default_max_retries")]
    pub max_retries: i64,
    pub order_idx: i64,
    pub created_at: String,
    pub updated_at: String,
    /// Runtime claim: who is currently executing this task (Slot ID or MCP session ID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_executor_id: Option<String>,
    /// Executor type: "pty_slot" or "manual_session"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_executor_type: Option<String>,
    /// When was this task claimed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<String>,
    /// Engineering flow: current phase (NULL = not a flow task)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_phase: Option<String>,
    /// Engineering flow: JSON context with phase artifacts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_context: Option<String>,
    /// Engineering flow: template name (e.g. "engineering")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_template: Option<String>,
}

fn default_max_retries() -> i64 { 2 }

/// Input for creating a board task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardTaskInput {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default, rename = "autoExecute")]
    pub auto_execute: Option<bool>,
    #[serde(default, rename = "promptTemplate")]
    pub prompt_template: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    /// Engineering flow: template name (e.g. "engineering"). Sets initial phase to "investigate".
    #[serde(default, rename = "flowTemplate")]
    pub flow_template: Option<String>,
}

/// Partial update for a board task
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardTaskInput {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default, rename = "autoExecute")]
    pub auto_execute: Option<bool>,
    #[serde(default, rename = "promptTemplate")]
    pub prompt_template: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub order_idx: Option<i64>,
    /// Claim executor ID (set via claim_board_task, cleared on status change away from running)
    #[serde(default)]
    pub claim_executor_id: Option<String>,
    /// Claim executor type: "pty_slot" or "manual_session"
    #[serde(default)]
    pub claim_executor_type: Option<String>,
    /// Engineering flow: current phase
    #[serde(default)]
    pub flow_phase: Option<String>,
    /// Engineering flow: JSON context
    #[serde(default)]
    pub flow_context: Option<String>,
    /// Engineering flow: template name
    #[serde(default)]
    pub flow_template: Option<String>,
}

// ============ Board Task Notes ============

/// Note type for board task progress tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardNoteType {
    Progress,
    Summary,
    Note,
}

impl BoardNoteType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BoardNoteType::Progress => "progress",
            BoardNoteType::Summary => "summary",
            BoardNoteType::Note => "note",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "progress" => Some(BoardNoteType::Progress),
            "summary" => Some(BoardNoteType::Summary),
            "note" => Some(BoardNoteType::Note),
            _ => None,
        }
    }
}

/// A note attached to a board task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTaskNote {
    pub id: String,
    pub task_id: String,
    pub content: String,
    pub note_type: BoardNoteType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: String,
}

/// Input for adding a note to a board task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddBoardTaskNoteInput {
    pub task_id: String,
    pub content: String,
    #[serde(default)]
    pub note_type: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
}

/// A board task with its notes included
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTaskWithNotes {
    #[serde(flatten)]
    pub task: BoardTask,
    pub notes: Vec<BoardTaskNote>,
}

// ============ Agent Questions (Pending Decisions) ============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentQuestionStatus {
    Pending,
    Answered,
    Dismissed,
}

impl AgentQuestionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Answered => "answered",
            Self::Dismissed => "dismissed",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "answered" => Some(Self::Answered),
            "dismissed" => Some(Self::Dismissed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentQuestion {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub question: String,
    pub context: String,
    pub status: AgentQuestionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Decision target: "user" (human) or "master" (daemon decision engine)
    #[serde(default = "default_target_user")]
    pub target: String,
    /// Structured options for decision (JSON array of choices)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<String>,
    /// Decision type: architecture/implementation/debug/investigation/risk/preference
    #[serde(default = "default_decision_type")]
    pub decision_type: String,
    /// Retry count for anti-loop protection
    #[serde(default)]
    pub retry_count: i64,
    /// Decision routing trace: JSON recording which tiers were visited and why
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_trace: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn default_target_user() -> String { "user".to_string() }
fn default_decision_type() -> String { "implementation".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentQuestionInput {
    pub question: String,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub slot_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Decision target: "user" or "master" (for Decision Engine)
    #[serde(default)]
    pub target: Option<String>,
    /// Structured options/choices (JSON array)
    #[serde(default)]
    pub options: Option<String>,
    /// Decision type: architecture/implementation/debug/investigation/risk/preference
    #[serde(default)]
    pub decision_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerAgentQuestionInput {
    pub id: String,
    pub answer: String,
}

// ============ Knowledge Base (Jarvis Memory) ============

/// A knowledge entry in the KB
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEntry {
    pub id: String,
    pub category: String,
    pub key: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    pub source: String,
    pub confidence: f64,
    pub access_count: i64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_task_id: Option<String>,
}

/// Input for remembering (upserting) knowledge
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KBRememberInput {
    pub category: String,
    pub key: String,
    pub summary: String,
    #[serde(default)]
    pub detail: Option<serde_json::Value>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// Result of a kb_remember operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KBRememberResult {
    pub entry: KnowledgeEntry,
    /// "created", "updated", "merged"
    pub action: String,
    /// If merged, the key of the entry that was merged into
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_key: Option<String>,
    /// Similarity score if merged
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
}

// ============ KB Operation Queue Types ============

/// Input for saving a KB operation (from consolidation plan)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KBOperation {
    pub operation: String,
    pub target_keys: Vec<String>,
    pub rationale: Option<String>,
}

/// Row from kb_operation_queue table
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KBOperationRow {
    pub id: String,
    pub plan_id: String,
    pub task_id: Option<String>,
    pub operation: String,
    pub target_keys: String,
    pub rationale: Option<String>,
    pub status: String,
    pub priority: i32,
    pub result: Option<String>,
    pub created_at: String,
    pub executed_at: Option<String>,
    pub error: Option<String>,
}

// ============ Skill Engine Types ============

/// Skill topic metadata (maps to skill_topics table)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillTopic {
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aka: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
    pub file_path: String,
    pub hit_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_hit_at: Option<String>,
    pub fragment_count: i64,
    pub total_lines: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// Phase 2: JSON-serialized SkillRequires (dependency declarations)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_json: Option<String>,
    /// Phase 3: JSON-serialized Vec<SkillAction> (executable actions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions_json: Option<String>,
    /// Phase 4: JSON-serialized Vec<ContextHook> (pre-flight probes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_hooks_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Skill block (section or fragment)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBlock {
    pub id: String,
    pub topic: String,
    pub block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub content: String,
    pub sort_order: i32,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Skill execution statistics (aggregated per action)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExecutionStat {
    pub action_id: String,
    pub total: i64,
    pub successes: i64,
    pub failures: i64,
    pub avg_duration_ms: Option<f64>,
    pub last_run: String,
}

/// Skill version snapshot (for rollback)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersion {
    pub id: i64,
    pub topic: String,
    pub content: String,
    pub checksum: String,
    pub created_at: String,
}

/// Skill FTS search result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSearchResult {
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A credential stored alongside a knowledge entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: String,
    pub knowledge_id: String,
    pub name: String,
    pub value_encrypted: String,
    pub created_at: String,
    pub updated_at: String,
}

// ============ Conversation Log ============

/// A conversation session (maps to a Claude Code JSONL file)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_id: Option<String>,
    pub source: String, // "claude_cli" | "pty"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonl_path: Option<String>,
    /// Parent conversation ID (for subagent sessions spawned by Task tool)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// Task ID: groups sessions that belong to the same logical task
    /// (survives Claude Code context compaction which creates new session IDs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub message_count: i64,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub status: String, // "active" | "completed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyzed_at: Option<String>,
    /// Deep analysis schema version (0 = never analyzed)
    #[serde(default)]
    pub analysis_version: i32,
    /// Retry count for deep analysis failures (capped at MAX_ANALYSIS_RETRIES)
    #[serde(default)]
    pub analysis_retries: i32,
    /// Checkpoint watermark: last message ID processed by incremental deep analysis
    #[serde(default)]
    pub deep_analyzed_message_id: i64,
    /// Conversation type: "pty" (default) or "router_chat" (Gemini sessions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_type: Option<String>,
    /// Classification: "user" | "meta" | "worker" | "subagent"
    #[serde(default = "default_conversation_type")]
    pub conversation_type: String,
}

fn default_conversation_type() -> String { "user".to_string() }

/// A message within a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String, // "user" | "assistant" | "tool_use" | "tool_result"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

/// A non-dialog system event from JSONL (turn_duration, compact_boundary, hook_progress, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationEvent {
    pub id: i64,
    pub session_id: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_data: Option<String>,
    pub timestamp: String,
}

// ============ Conversation Tool Calls (Audit) ============

/// A structured tool call record extracted from JSONL tool_use/tool_result pairs.
/// Used for audit trail (Summary-to-Drilldown architecture).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub id: String,              // tool_use_id from Claude API
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<i64>, // FK to conversation_messages
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<String>,
    pub status: String,          // pending, success, error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    pub timestamp: String,
}

// ============ Slot Task History ============

/// A task dispatched to a slot by the daemon (for tracking history)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotTask {
    pub id: String,
    pub slot_id: String,
    /// Task type: "realtime_extract", "deep_analysis", "kb_gc", etc.
    pub task_type: String,
    /// Status: "pending", "running", "completed", "failed"
    pub status: String,
    /// First ~200 chars of the prompt sent to the slot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_summary: Option<String>,
    /// JSON array of source session IDs that triggered this task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sessions: Option<String>,
    /// Number of KB entries produced by this task
    #[serde(default)]
    pub output_count: i64,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The conversation ID created on the slot for this task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

// ============ AIOps Incident ============

/// Severity levels for automated incidents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentSeverity {
    /// Service down, data loss risk.
    Critical,
    /// Feature degraded, deploy failure.
    High,
    /// Disk >80%, slow response, etc.
    Warning,
}

impl std::fmt::Display for IncidentSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "critical"),
            Self::High => write!(f, "high"),
            Self::Warning => write!(f, "warning"),
        }
    }
}

/// Source of an automated incident.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentSource {
    HealthCheck,
    DeployCenter,
    Sentry,
    Manual,
}

impl std::fmt::Display for IncidentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HealthCheck => write!(f, "health_check"),
            Self::DeployCenter => write!(f, "deploy_center"),
            Self::Sentry => write!(f, "sentry"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

/// An automated incident detected by a sensor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionIncident {
    pub id: String,
    pub severity: IncidentSeverity,
    pub source: IncidentSource,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    pub raw_payload: serde_json::Value,
    pub created_at: String,
}

/// Row from the incidents table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentRow {
    pub id: String,
    pub severity: String,
    pub source: String,
    pub title: String,
    pub description: String,
    pub server_id: Option<String>,
    pub board_task_id: Option<String>,
    pub dedupe_key: String,
    pub created_at: String,
}

// ============ Infrastructure Server Registry ============

/// A server in the infrastructure registry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InfraServer {
    pub id: String,
    pub name: String,
    pub provider: String, // gcp, aliyun, self-hosted, bandwagon
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>, // build, deploy, gpu, vpn, production
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// HTTP(S) endpoint for deploy-agent health checks (Probe 5 in reachability).
    /// If set, reachability tool uses this URL instead of hardcoded map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_endpoint: Option<String>,
}

/// Parsed SSH connection target from InfraServer description
#[derive(Debug, Clone)]
pub struct SshTarget {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    /// "lan", "tailscale", or "public"
    pub via: String,
}

impl InfraServer {
    /// Parse SSH targets from description field, ordered by priority (LAN > Tailscale > public).
    ///
    /// Matches patterns like:
    /// - `ssh user@192.168.1.100 (密码 pass`
    /// - `ssh -p 2222 user@198.51.100.1 (密码 pass`
    /// - `ssh user@100.64.0.1`
    pub fn parse_ssh_targets(&self) -> Vec<SshTarget> {
        let desc = match &self.description {
            Some(d) => d,
            None => return Vec::new(),
        };

        let re = regex::Regex::new(
            r"ssh\s+(?:-p\s+(\d+)\s+)?(\w[\w.-]*)@([\d.]+)(?:\s+\([^)]*(?:密码|password)\s+([^\s,)]+))?"
        ).unwrap();

        let mut targets = Vec::new();
        for cap in re.captures_iter(desc) {
            let port = cap.get(1)
                .and_then(|m| m.as_str().parse::<u16>().ok())
                .unwrap_or(22);
            let user = cap[2].to_string();
            let host = cap[3].to_string();
            let password = cap.get(4).map(|m| m.as_str().to_string());

            let via = if self.lan.as_deref() == Some(host.as_str()) {
                "lan"
            } else if host.starts_with("100.") {
                "tailscale"
            } else if host.starts_with("192.168.") || host.starts_with("10.") || host.starts_with("172.") {
                "lan"
            } else {
                "public"
            };

            targets.push(SshTarget {
                user,
                host,
                port,
                password,
                via: via.to_string(),
            });
        }

        // Sort: lan first, tailscale second, public last
        targets.sort_by_key(|t| match t.via.as_str() {
            "lan" => 0,
            "tailscale" => 1,
            _ => 2,
        });

        targets
    }
}

/// Infrastructure configuration (loaded from servers.yaml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraConfig {
    #[serde(default)]
    pub servers: Vec<InfraServer>,
}

impl InfraConfig {
    /// Load from YAML file, returns empty config if file doesn't exist
    pub fn load(path: &std::path::Path) -> Self {
        if !path.exists() {
            return Self { servers: Vec::new() };
        }
        match std::fs::read_to_string(path) {
            Ok(content) => serde_yaml::from_str(&content).unwrap_or(Self { servers: Vec::new() }),
            Err(_) => Self { servers: Vec::new() },
        }
    }

    /// Get server by ID
    pub fn get(&self, id: &str) -> Option<&InfraServer> {
        self.servers.iter().find(|s| s.id == id)
    }

    /// Filter servers by role
    pub fn by_role(&self, role: &str) -> Vec<&InfraServer> {
        self.servers.iter().filter(|s| s.roles.iter().any(|r| r == role)).collect()
    }

    /// Filter servers by provider
    pub fn by_provider(&self, provider: &str) -> Vec<&InfraServer> {
        self.servers.iter().filter(|s| s.provider == provider).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_roundtrip() {
        let statuses = [
            TaskStatus::Queued,
            TaskStatus::Running,
            TaskStatus::Done,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ];

        for status in statuses {
            let s = status.as_str();
            let parsed = TaskStatus::from_str(s).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_event_type_roundtrip() {
        let types = [
            EventType::TaskCreated,
            EventType::TaskStarted,
            EventType::TaskProgress,
            EventType::TaskDone,
            EventType::TaskFailed,
            EventType::TaskCancelled,
        ];

        for t in types {
            let s = t.as_str();
            let parsed = EventType::from_str(s).unwrap();
            assert_eq!(t, parsed);
        }
    }

    #[test]
    fn test_task_serialization() {
        let task = Task {
            id: "task-123".to_string(),
            role: "worker".to_string(),
            prompt: "Do something".to_string(),
            status: TaskStatus::Queued,
            slot_id: None,
            session_id: None,
            result: None,
            error: None,
            created_at: 1234567890,
            started_at: None,
            finished_at: None,
        };

        let json = serde_json::to_string(&task).unwrap();
        let parsed: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(task.id, parsed.id);
        assert_eq!(task.role, parsed.role);
        assert_eq!(task.status, parsed.status);
    }

    #[test]
    fn test_slot_config_serialization() {
        let config = SlotConfig {
            id: "slot-1".to_string(),
            role: "worker".to_string(),
            description: "A worker slot".to_string(),
            cwd: Some("/path/to/work".to_string()),
            mcp_config: None,
            auto_start: Some(true),
            dangerously_skip_permissions: None,
            traits: vec![],
            env: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"id\":\"slot-1\""));
        assert!(json.contains("\"autoStart\":true"));
    }

    #[test]
    fn test_parse_ssh_targets_privatecloud() {
        let server = InfraServer {
            id: "privatecloud".into(),
            name: "私有云构建机".into(),
            provider: "self-hosted".into(),
            host: Some("198.51.100.1".into()),
            lan: Some("192.168.1.100".into()),
            location: None,
            roles: vec![],
            tags: vec![],
            description: Some(
                "Build server. Access: \
                 1) LAN: ssh testuser@192.168.1.100 (密码 testpass, 同局域网最快) \
                 2) Tailscale: ssh testuser@100.64.0.1 (testuser-buildbox) \
                 3) Tunnel: ssh -p 2222 testuser@198.51.100.1 (密码 testpass, 公网)"
                    .into(),
            ),
            health_endpoint: None,
        };

        let targets = server.parse_ssh_targets();
        assert_eq!(targets.len(), 3);

        // LAN first
        assert_eq!(targets[0].via, "lan");
        assert_eq!(targets[0].host, "192.168.1.100");
        assert_eq!(targets[0].user, "testuser");
        assert_eq!(targets[0].port, 22);
        assert_eq!(targets[0].password.as_deref(), Some("testpass"));

        // Tailscale second
        assert_eq!(targets[1].via, "tailscale");
        assert_eq!(targets[1].host, "100.64.0.1");

        // Public last
        assert_eq!(targets[2].via, "public");
        assert_eq!(targets[2].host, "198.51.100.1");
        assert_eq!(targets[2].port, 2222);
        assert_eq!(targets[2].password.as_deref(), Some("testpass"));
    }

    #[test]
    fn test_parse_ssh_targets_windows() {
        let server = InfraServer {
            id: "win-3090ti".into(),
            name: "Windows".into(),
            provider: "self-hosted".into(),
            host: Some("192.168.1.101".into()),
            lan: None,
            location: None,
            roles: vec![],
            tags: vec![],
            description: Some(
                "访问: 1) LAN: ssh testuser@192.168.1.101 (密码 testpass) \
                 2) Tailscale: ssh testuser@100.64.0.2 (testuser-gpu)"
                    .into(),
            ),
            health_endpoint: None,
        };

        let targets = server.parse_ssh_targets();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].user, "testuser");
        assert_eq!(targets[0].host, "192.168.1.101");
        // Tailscale
        assert_eq!(targets[1].via, "tailscale");
        assert_eq!(targets[1].host, "100.64.0.2");
    }

    #[test]
    fn test_parse_ssh_targets_no_description() {
        let server = InfraServer {
            id: "test".into(),
            name: "Test".into(),
            provider: "gcp".into(),
            host: None,
            lan: None,
            location: None,
            roles: vec![],
            tags: vec![],
            description: None,
            health_endpoint: None,
        };
        assert!(server.parse_ssh_targets().is_empty());
    }
}
