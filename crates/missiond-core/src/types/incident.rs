use serde::{Deserialize, Serialize};

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
    /// PTY slot detected an anomaly (e.g., MCP tool unavailable)
    PtySlot,
}

impl std::fmt::Display for IncidentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HealthCheck => write!(f, "health_check"),
            Self::DeployCenter => write!(f, "deploy_center"),
            Self::Sentry => write!(f, "sentry"),
            Self::Manual => write!(f, "manual"),
            Self::PtySlot => write!(f, "pty_slot"),
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
