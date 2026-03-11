use serde::{Deserialize, Serialize};

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
    /// DAG dependency: IDs of tasks that must be done before this task can execute
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Lease expiration for running tasks (heartbeat mechanism).
    /// Tasks running past this time are considered stale and recoverable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<String>,
    /// Alert fingerprint for AIOps deduplication (e.g. "health_check:ECS 健康检查失败").
    /// Used to find existing open tasks and aggregate repeated alerts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
}

fn default_max_retries() -> i64 { 2 }

/// Compact board task for search results (high density, low token cost)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactBoardTask {
    pub id: String,
    pub title: String,
    pub status: BoardTaskStatus,
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// Board task with optional children tree (for includeChildren mode)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTaskWithContext {
    #[serde(flatten)]
    pub task: BoardTask,
    pub notes: Vec<BoardTaskNote>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<BoardTaskWithContext>>,
}

/// Search results wrapper with meta info to prevent context overflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSearchResult {
    pub meta: BoardSearchMeta,
    pub data: Vec<CompactBoardTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSearchMeta {
    pub total: usize,
    pub returned: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Input for board search
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSearchInput {
    pub query: Option<String>,
    pub project: Option<String>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub parent_id: Option<String>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub include_hidden: bool,
}

/// Result of checking whether a task's DAG dependencies are satisfied
#[derive(Debug, Clone)]
pub enum DependencyStatus {
    /// All dependencies are done
    Ready,
    /// Some dependencies are still open/running
    Pending,
    /// A dependency has failed or been skipped — block the downstream task
    Blocked(String),
}

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
    /// DAG dependency: IDs of tasks that must complete before this task
    #[serde(default, rename = "dependsOn")]
    pub depends_on: Option<Vec<String>>,
    /// Alert fingerprint for AIOps deduplication (set internally, not from MCP)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
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
    /// DAG dependency: IDs of tasks that must complete before this task
    #[serde(default, rename = "dependsOn")]
    pub depends_on: Option<Vec<String>>,
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
