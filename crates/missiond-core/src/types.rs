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
