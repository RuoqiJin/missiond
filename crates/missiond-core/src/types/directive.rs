//! Directive / Plan / Workflow types.
//!
//! Three tables of the directive-layer pillar:
//! - `directive`: user utterance compiled into a lisp-coded directive
//! - `plan`: sexp execution DAG compiled from a directive, pinned to a board_task
//! - `workflow`: reusable template distilled from a successful plan
//!
//! See migration `20260420000000_directive_plan_workflow.sql` for schema truth.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

// ============================================================================
// DirectiveStatus
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveStatus {
    Draft,
    Refining,
    Approved,
    Compiled,
    Archived,
}

impl DirectiveStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Refining => "refining",
            Self::Approved => "approved",
            Self::Compiled => "compiled",
            Self::Archived => "archived",
        }
    }
}

impl fmt::Display for DirectiveStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DirectiveStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "refining" => Ok(Self::Refining),
            "approved" => Ok(Self::Approved),
            "compiled" => Ok(Self::Compiled),
            "archived" => Ok(Self::Archived),
            other => Err(format!("unknown DirectiveStatus: {}", other)),
        }
    }
}

// ============================================================================
// PlanStatus
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Draft,
    AwaitingApproval,
    Approved,
    Executing,
    Succeeded,
    Failed,
    Superseded,
}

impl PlanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Approved => "approved",
            Self::Executing => "executing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }
}

impl fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PlanStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "approved" => Ok(Self::Approved),
            "executing" => Ok(Self::Executing),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "superseded" => Ok(Self::Superseded),
            other => Err(format!("unknown PlanStatus: {}", other)),
        }
    }
}

// ============================================================================
// Directive
// ============================================================================

/// A user utterance compiled into a lisp-coded directive.
///
/// Corresponds to a row in the `directive` table. The `(id, version)` pair is unique —
/// a single conceptual directive may have multiple versions (refining history).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directive {
    pub id: Uuid,
    pub utterance_text: String,
    pub sexp_text: String,
    pub version: i32,
    pub status: DirectiveStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiler_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references_json: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<DateTime<Utc>>,
}

// ============================================================================
// Plan
// ============================================================================

/// An sexp execution DAG compiled from a directive, pinned to a board_task.
///
/// Corresponds to a row in the `plan` table. `(board_task_id, version)` is unique.
/// `board_task_id` is TEXT (board_tasks.id is TEXT — see 20260318000000_init.sql).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub board_task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_directive_id: Option<Uuid>,
    pub version: i32,
    pub sexp_text: String,
    pub sexp_hash: String,
    pub status: PlanStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiler_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiled_from: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

// ============================================================================
// Workflow
// ============================================================================

/// A reusable template distilled from a successful plan.
///
/// Corresponds to a row in the `workflow` table. `name` is unique.
/// `avg_cost_usd` is stored as NUMERIC(10,4); we surface it as `f64` here by casting
/// in the PG layer (no rust_decimal dependency).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: Uuid,
    pub name: String,
    pub sexp_text: String,
    pub match_rules: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learned_from: Option<Uuid>,
    pub executions: i32,
    pub success_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
