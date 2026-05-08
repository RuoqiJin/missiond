use serde::{Deserialize, Serialize};

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

fn default_target_user() -> String {
    "user".to_string()
}
fn default_decision_type() -> String {
    "implementation".to_string()
}

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
