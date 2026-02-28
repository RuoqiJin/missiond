//! Types for Claude Code Tasks Integration
//!
//! Mirrors TypeScript definitions from packages/missiond/src/cc-tasks/types.ts

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============ Claude Code Native Formats ============

/// Entry in sessions-index.json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CCSessionIndexEntry {
    pub session_id: String,
    pub full_path: String,
    pub file_mtime: i64,
    pub first_prompt: String,
    pub summary: String,
    pub message_count: u32,
    pub created: String,
    pub modified: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub project_path: String,
    pub is_sidechain: bool,
}

/// sessions-index.json root structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CCSessionIndex {
    pub version: u32,
    pub entries: Vec<CCSessionIndexEntry>,
}

/// Task item in todos array
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CCTask {
    pub content: String,
    pub status: CCTaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CCTaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl CCTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CCTaskStatus::Pending => "pending",
            CCTaskStatus::InProgress => "in_progress",
            CCTaskStatus::Completed => "completed",
        }
    }
}

/// A line in the .jsonl conversation file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CCMessageLine {
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    pub is_sidechain: bool,
    pub cwd: String,
    pub session_id: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub message: CCMessage,
    pub uuid: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub todos: Option<Vec<CCTask>>,
}

/// Token usage from Claude API response
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub cache_creation_input_tokens: i64,
    #[serde(default)]
    pub cache_read_input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
}

/// Message content in JSONL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CCMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

// ============ Aggregated Views ============

/// Enriched session with current tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CCSession {
    pub session_id: String,
    pub project_path: String,
    /// Last 2 path segments
    pub project_name: String,
    pub summary: String,
    pub modified: DateTime<Utc>,
    pub created: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub tasks: Vec<CCTask>,
    pub message_count: u32,
    /// Updated within last 5 minutes
    pub is_active: bool,
    pub full_path: String,
}

/// Task change event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CCTaskChangeEvent {
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub previous_tasks: Vec<CCTask>,
    pub current_tasks: Vec<CCTask>,
    pub added: Vec<CCTask>,
    pub removed: Vec<CCTask>,
    pub status_changed: Vec<TaskStatusChange>,
    pub timestamp: DateTime<Utc>,
}

/// Task status change
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusChange {
    pub task: CCTask,
    pub previous_status: CCTaskStatus,
}

/// Global tasks overview
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CCTasksOverview {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub tasks_by_status: TasksByStatus,
    pub sessions_with_tasks: usize,
    pub recent_changes: Vec<CCTaskChangeEvent>,
}

/// Tasks count by status
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TasksByStatus {
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
}

/// In-progress task with session context
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CCInProgressTask {
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub summary: String,
    pub task: CCTask,
    pub modified: DateTime<Utc>,
}

/// Task diff result
#[derive(Debug, Clone, Default)]
pub struct TaskDiff {
    pub added: Vec<CCTask>,
    pub removed: Vec<CCTask>,
    pub status_changed: Vec<TaskStatusChange>,
}

impl TaskDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.status_changed.is_empty()
    }
}
