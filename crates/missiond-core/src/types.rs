//! Core types for missiond
//!
//! Mirrors the TypeScript definitions in packages/missiond/src/types.ts

use serde::{Deserialize, Serialize};

// ============ Slot Config ============

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
    Done,
}

impl BoardTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BoardTaskStatus::Open => "open",
            BoardTaskStatus::Done => "done",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "open" => Some(BoardTaskStatus::Open),
            "done" => Some(BoardTaskStatus::Done),
            _ => None,
        }
    }
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
    pub order_idx: i64,
    pub created_at: String,
    pub updated_at: String,
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
    pub order_idx: Option<i64>,
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
    pub message_count: i64,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub status: String, // "active" | "completed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyzed_at: Option<String>,
}

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
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"id\":\"slot-1\""));
        assert!(json.contains("\"autoStart\":true"));
    }
}
