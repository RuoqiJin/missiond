//! Request-local review_packet projection for mission_request.
//!
//! V3 authority: .missiond/v3/missiond-blueprint.lisp ::
//! unified-entry review-packet. This module is pure projection from
//! request-local artifact existence, latest projection target, and review
//! event checkpoint; it never approves intent/plan and never dispatches work.

use missiond_core::util::safe_byte_truncate;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::request_artifacts::{path_json, RequestMode, RequestPaths};

// ───────────────────────────────────────────────────────────────────────
// Review packet — V3 unified-entry projection. Pure derivation from
// request-local artifact existence + latest projection target/preview.
// Never approves intent or plan, never dispatches workstation work.
// ───────────────────────────────────────────────────────────────────────

pub(super) const REVIEW_PREVIEW_MAX_BYTES: usize = 480;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ArtifactExistence {
    pub(super) request: bool,
    pub(super) intent_alignment: bool,
    pub(super) plan: bool,
}

pub(super) fn read_artifact_existence(paths: &RequestPaths) -> ArtifactExistence {
    ArtifactExistence {
        request: paths.request.exists(),
        intent_alignment: paths.intent_alignment.exists(),
        plan: paths.plan.exists(),
    }
}

pub(super) struct ReviewPacketInputs<'a> {
    pub(super) mode: RequestMode,
    pub(super) paths: &'a RequestPaths,
    pub(super) existence: ArtifactExistence,
    pub(super) projection_target: Option<&'static str>,
    pub(super) fallback_preview: Option<&'a str>,
    pub(super) execute_requested: bool,
    pub(super) review_checkpoint: Option<ReviewEventCheckpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReviewState {
    Received,
    IntentDrafting,
    AwaitingIntentApproval,
    AwaitingPlanApproval,
    AwaitingExecution,
    ExecuteRequested,
}

impl ReviewState {
    fn wire(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::IntentDrafting => "intent_drafting",
            Self::AwaitingIntentApproval => "awaiting_intent_approval",
            Self::AwaitingPlanApproval => "awaiting_plan_approval",
            Self::AwaitingExecution => "awaiting_execution",
            Self::ExecuteRequested => "execute_requested",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReviewEventCheckpoint {
    PlanApproved,
    ExecuteRequested,
}

pub(super) fn latest_review_event_checkpoint(
    event_texts: &[String],
) -> Option<ReviewEventCheckpoint> {
    for text in event_texts.iter().rev() {
        if text.contains(":decision :execute_plan") {
            if text.contains(":outcome :dispatched") {
                return Some(ReviewEventCheckpoint::ExecuteRequested);
            }
            continue;
        }
        if text.contains(":decision :approve_plan") {
            if text.contains(":outcome :dispatched") {
                return Some(ReviewEventCheckpoint::PlanApproved);
            }
            continue;
        }
        if text.contains(":decision :reject_plan")
            || text.contains(":decision :ask_question")
            || text.contains(":decision :approve_intent")
            || text.contains(":decision :reject_intent")
        {
            return None;
        }
    }
    None
}

pub(super) fn classify_review_state(
    existence: ArtifactExistence,
    projection_target: Option<&'static str>,
    execute_requested: bool,
    review_checkpoint: Option<ReviewEventCheckpoint>,
) -> (ReviewState, &'static str) {
    if existence.plan
        && (execute_requested || review_checkpoint == Some(ReviewEventCheckpoint::ExecuteRequested))
    {
        (ReviewState::ExecuteRequested, "plan")
    } else if existence.plan && review_checkpoint == Some(ReviewEventCheckpoint::PlanApproved) {
        (ReviewState::AwaitingExecution, "plan")
    } else if existence.plan {
        (ReviewState::AwaitingPlanApproval, "plan")
    } else if existence.intent_alignment {
        (ReviewState::AwaitingIntentApproval, "intent_alignment")
    } else if let Some(target) = projection_target {
        (ReviewState::IntentDrafting, target)
    } else {
        (ReviewState::Received, "request")
    }
}

pub(super) fn review_state_messages(state: ReviewState) -> (&'static str, &'static str, bool) {
    match state {
        ReviewState::ExecuteRequested => (
            "Plan execution requested; observe execution status through MissionD.",
            "observe execution status through mission_request status and task receipts",
            true,
        ),
        ReviewState::AwaitingPlanApproval => (
            "Review plan.lisp, then answer through mission_request respond with approve_plan, reject_plan, or ask_question.",
            "call mission_request respond with response=approve_plan, reject_plan, or ask_question",
            false,
        ),
        ReviewState::AwaitingExecution => (
            "Plan is approved. Dispatch only through mission_request respond with execute_plan and execute=true.",
            "call mission_request respond with response=execute_plan + execute=true",
            true,
        ),
        ReviewState::AwaitingIntentApproval => (
            "Review intent-alignment.lisp, then answer through mission_request respond with approve_intent, reject_intent, or ask_question.",
            "call mission_request respond with response=approve_intent, reject_intent, or ask_question",
            false,
        ),
        ReviewState::IntentDrafting => (
            "Drafting; pipeline projection targeted an artifact but it has not landed yet. Re-poll mission_request status.",
            "wait for projection to land, then re-poll mission_request status",
            false,
        ),
        ReviewState::Received => (
            "Request received; advance pipeline to draft intent or plan.",
            "call mission_request advance to drive the next pipeline stage",
            false,
        ),
    }
}

pub(super) fn allowed_responses_for(mode: RequestMode, state: ReviewState) -> Vec<&'static str> {
    match (mode, state) {
        (RequestMode::HumanInteractive, ReviewState::AwaitingIntentApproval) => {
            vec!["approve_intent", "reject_intent", "ask_question"]
        }
        (RequestMode::HumanInteractive, ReviewState::AwaitingPlanApproval) => {
            vec!["approve_plan", "reject_plan", "ask_question"]
        }
        (RequestMode::TrustedAgent, ReviewState::AwaitingIntentApproval) => {
            vec!["approve_intent", "ask_question"]
        }
        (RequestMode::TrustedAgent, ReviewState::AwaitingPlanApproval) => {
            vec!["approve_plan", "ask_question"]
        }
        (_, ReviewState::AwaitingExecution) => vec!["execute_plan", "ask_question"],
        (_, ReviewState::ExecuteRequested) => vec!["observe"],
        _ => vec!["observe"],
    }
}

pub(super) fn artifact_path_for_kind<'a>(paths: &'a RequestPaths, kind: &str) -> &'a Path {
    match kind {
        "plan" => paths.plan.as_path(),
        "intent_alignment" => paths.intent_alignment.as_path(),
        _ => paths.request.as_path(),
    }
}

pub(super) fn build_review_artifact_preview<F>(
    target_path: &Path,
    artifact_exists: bool,
    fallback: Option<&str>,
    read_file: F,
    max_bytes: usize,
) -> Option<String>
where
    F: Fn(&Path) -> Option<String>,
{
    if artifact_exists {
        if let Some(text) = read_file(target_path) {
            return Some(safe_byte_truncate(&text, max_bytes).to_string());
        }
    }
    fallback.map(|s| safe_byte_truncate(s, max_bytes).to_string())
}

#[derive(Debug, Clone)]
pub(super) struct RequestProjection {
    state: ReviewState,
    artifact_kind: &'static str,
    artifact_path: PathBuf,
    artifact_exists: bool,
    artifact_preview: Option<String>,
    prompt: &'static str,
    allowed_responses: Vec<&'static str>,
    next_action: &'static str,
    execute_allowed: bool,
}

impl RequestProjection {
    pub(super) fn to_review_packet_json(&self) -> Value {
        json!({
            "state": self.state.wire(),
            "artifact_kind": self.artifact_kind,
            "artifact_path": path_json(&self.artifact_path),
            "artifact_exists": self.artifact_exists,
            "artifact_preview": self.artifact_preview.clone(),
            "prompt": self.prompt,
            "allowed_responses": self.allowed_responses.clone(),
            "next_action": self.next_action,
            "execute_allowed": self.execute_allowed,
        })
    }
}

pub(super) fn derive_request_projection<F>(
    inputs: &ReviewPacketInputs<'_>,
    read_file: F,
) -> RequestProjection
where
    F: Fn(&Path) -> Option<String>,
{
    let (state, artifact_kind) = classify_review_state(
        inputs.existence,
        inputs.projection_target,
        inputs.execute_requested,
        inputs.review_checkpoint,
    );
    let (prompt, next_action, execute_allowed) = review_state_messages(state);
    let target_path = artifact_path_for_kind(inputs.paths, artifact_kind);
    let artifact_exists = match artifact_kind {
        "plan" => inputs.existence.plan,
        "intent_alignment" => inputs.existence.intent_alignment,
        _ => inputs.existence.request,
    };
    let preview = build_review_artifact_preview(
        target_path,
        artifact_exists,
        inputs.fallback_preview,
        read_file,
        REVIEW_PREVIEW_MAX_BYTES,
    );
    let allowed = allowed_responses_for(inputs.mode, state);
    RequestProjection {
        state,
        artifact_kind,
        artifact_path: target_path.to_path_buf(),
        artifact_exists,
        artifact_preview: preview,
        prompt,
        allowed_responses: allowed,
        next_action,
        execute_allowed,
    }
}

#[cfg(test)]
pub(super) fn derive_review_packet<F>(inputs: &ReviewPacketInputs<'_>, read_file: F) -> Value
where
    F: Fn(&Path) -> Option<String>,
{
    derive_request_projection(inputs, read_file).to_review_packet_json()
}

pub(super) fn parse_execute_requested(args: &Value) -> bool {
    args.get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || args
            .get("execute_after_approval")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

pub(super) fn extract_mode_from_request_lisp(text: &str) -> RequestMode {
    if text.contains(":mode :trusted-agent") {
        RequestMode::TrustedAgent
    } else {
        RequestMode::HumanInteractive
    }
}
