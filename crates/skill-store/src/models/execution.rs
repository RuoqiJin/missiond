use serde::{Deserialize, Serialize};

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
