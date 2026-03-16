use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub id: String,
    pub user_id: String,
    pub skill_id: String,
    pub status: ExecStatus,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost: f64,
    pub creator_revenue: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecStatus {
    Success,
    BlockedInjection,
    BlockedLeak,
    Failed,
}

impl ExecStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Success => "success",
            Self::BlockedInjection => "blocked_injection",
            Self::BlockedLeak => "blocked_leak",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "success" => Self::Success,
            "blocked_injection" => Self::BlockedInjection,
            "blocked_leak" => Self::BlockedLeak,
            _ => Self::Failed,
        }
    }
}
