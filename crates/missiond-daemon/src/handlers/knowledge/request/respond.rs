//! Review-response adapter for mission_request.
//!
//! V3 authority: .missiond/v3/missiond-blueprint.lisp ::
//! unified-entry review-response. This module routes explicit user answers
//! from review_packet into mission_directive / mission_plan / unified_entry
//! without bypassing their gates, and records request-local review events.

use anyhow::Result;
use missiond_core::types::{CreateBoardTaskInput, PlanStatus};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::handlers::knowledge::file_artifacts::atomic_write_artifact;
use crate::state::AppState;

use super::request_artifacts::{
    build_artifact_existence, build_artifact_paths_json, nonblank, now_rfc3339, path_json,
    projection_to_json, request_paths_for, resolve_request_project_root, run_projection,
    sanitize_request_id, tool_result_payload, ProjectionStatus, RequestMode, RequestPaths,
};
use super::review_packet::{
    derive_review_packet, extract_mode_from_request_lisp, latest_review_event_checkpoint,
    read_artifact_existence, ReviewPacketInputs,
};
use super::{compat_write_requested, lisp_string, EVENT_SCHEMA};

// ───────────────────────────────────────────────────────────────────────
// review-response adapter — V3 unified-entry continuation. mission_request
// is the user-facing surface for answering a review_packet; it never
// silently approves, never bypasses mission_directive / mission_plan
// gates, and never spawns workstation work directly.
//
// Lisp authority:
//   .missiond/v3/missiond-blueprint.lisp :: unified-entry :: review-response
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RespondDecision {
    ApproveIntent,
    RejectIntent,
    AskQuestion,
    ApprovePlan,
    RejectPlan,
    ExecutePlan,
}

impl RespondDecision {
    pub(super) fn wire(self) -> &'static str {
        match self {
            Self::ApproveIntent => "approve_intent",
            Self::RejectIntent => "reject_intent",
            Self::AskQuestion => "ask_question",
            Self::ApprovePlan => "approve_plan",
            Self::RejectPlan => "reject_plan",
            Self::ExecutePlan => "execute_plan",
        }
    }

    pub(super) fn requires_directive_ref(self) -> bool {
        matches!(self, Self::ApproveIntent | Self::RejectIntent)
    }

    pub(super) fn requires_plan_ref(self) -> bool {
        matches!(
            self,
            Self::ApprovePlan | Self::RejectPlan | Self::ExecutePlan
        )
    }

    /// Record-only routes never mutate directive/plan approval state and
    /// only persist a request-local review event so the user decision
    /// remains auditable.
    pub(super) fn record_only(self) -> bool {
        matches!(
            self,
            Self::RejectIntent | Self::RejectPlan | Self::AskQuestion
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RespondParseError {
    Missing,
    Unknown(String),
}

impl RespondParseError {
    pub(super) fn into_tool_error(self) -> ToolError {
        match self {
            Self::Missing => ToolError::new(
                error_codes::MISSING_PARAM,
                "respond requires `response` (or `decision`)",
            )
            .with_suggestion(
                "valid: approve_intent|reject_intent|ask_question|approve_plan|reject_plan|execute_plan",
            ),
            Self::Unknown(raw) => ToolError::new(
                error_codes::INVALID_PARAM,
                format!("unknown respond decision `{}`", raw),
            )
            .with_suggestion(
                "valid: approve_intent|reject_intent|ask_question|approve_plan|reject_plan|execute_plan",
            ),
        }
    }
}

/// Pure decision parse — accepts `response` or `decision`. Pulled out so
/// unit tests can pin the canonical wire vocabulary without an AppState.
pub(super) fn parse_respond_decision(
    args: &Value,
) -> std::result::Result<RespondDecision, RespondParseError> {
    let raw = nonblank(args.get("response"))
        .or_else(|| nonblank(args.get("decision")))
        .ok_or(RespondParseError::Missing)?;
    match raw.as_str() {
        "approve_intent" => Ok(RespondDecision::ApproveIntent),
        "reject_intent" => Ok(RespondDecision::RejectIntent),
        "ask_question" => Ok(RespondDecision::AskQuestion),
        "approve_plan" => Ok(RespondDecision::ApprovePlan),
        "reject_plan" => Ok(RespondDecision::RejectPlan),
        "execute_plan" => Ok(RespondDecision::ExecutePlan),
        _ => Err(RespondParseError::Unknown(raw)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DirectiveRef {
    pub(super) id: String,
    pub(super) version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlanRef {
    pub(super) id: String,
}

/// Best-effort scan of a Lisp artifact for `:<key> "<uuid>"`. Pure helper —
/// no IO, no regex crate. Picks the first occurrence so callers keep the
/// canonical persisted ref ahead of any later debug noise.
pub(super) fn extract_lisp_keyword_string(text: &str, key: &str) -> Option<String> {
    let needle = format!(":{}", key);
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(&needle) {
        let abs = cursor + found;
        let after = &text[abs + needle.len()..];
        let trimmed = after.trim_start_matches([' ', '\t', '\r', '\n']);
        if let Some(stripped) = trimmed.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                let val = &stripped[..end];
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
        cursor = abs + needle.len();
    }
    None
}

pub(super) fn extract_lisp_keyword_int(text: &str, key: &str) -> Option<i32> {
    let needle = format!(":{}", key);
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(&needle) {
        let abs = cursor + found;
        let after = &text[abs + needle.len()..];
        let trimmed = after.trim_start_matches([' ', '\t', '\r', '\n']);
        let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<i32>() {
            return Some(n);
        }
        cursor = abs + needle.len();
    }
    None
}

pub(super) fn is_uuid_shaped(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

pub(super) fn resolve_directive_ref(
    args: &Value,
    intent_alignment_text: Option<&str>,
) -> Option<DirectiveRef> {
    let id =
        nonblank(args.get("approved_directive_id")).or_else(|| nonblank(args.get("directive_id")));
    let version = args
        .get("directive_version")
        .and_then(|v| v.as_i64())
        .map(|n| n as i32);

    let (id, version) = match (id, version) {
        (Some(id), Some(v)) => (id, v),
        _ => match intent_alignment_text.and_then(extract_directive_ref_from_artifact) {
            Some(parsed) => (parsed.id, parsed.version),
            None => return None,
        },
    };
    Some(DirectiveRef { id, version })
}

pub(super) fn extract_directive_ref_from_artifact(text: &str) -> Option<DirectiveRef> {
    let id = match extract_lisp_keyword_string(text, "directive_id") {
        Some(id) => id,
        None => extract_lisp_keyword_string(text, "id").filter(|id| is_uuid_shaped(id))?,
    };
    let version = extract_lisp_keyword_int(text, "directive_version")
        .or_else(|| extract_lisp_keyword_int(text, "version"))?;
    Some(DirectiveRef { id, version })
}

pub(super) fn resolve_plan_ref(
    args: &Value,
    plan_text: Option<&str>,
    event_texts: &[String],
) -> Option<PlanRef> {
    if let Some(id) =
        nonblank(args.get("approved_plan_id")).or_else(|| nonblank(args.get("plan_id")))
    {
        return Some(PlanRef { id });
    }
    plan_text
        .and_then(extract_plan_ref_from_artifact)
        .or_else(|| extract_latest_plan_ref_from_events(event_texts))
}

pub(super) fn extract_plan_ref_from_artifact(text: &str) -> Option<PlanRef> {
    if let Some(id) = extract_lisp_keyword_string(text, "plan_id") {
        return Some(PlanRef { id });
    }
    // Request-local plan.lisp may contain nested node ids such as
    // `(:id "root" ...)`; never treat those as persisted plan refs.
    extract_lisp_keyword_string(text, "id")
        .filter(|id| is_uuid_shaped(id))
        .map(|id| PlanRef { id })
}

pub(super) fn extract_latest_plan_ref_from_events(event_texts: &[String]) -> Option<PlanRef> {
    event_texts
        .iter()
        .rev()
        .find_map(|text| extract_lisp_keyword_string(text, "plan_id").map(|id| PlanRef { id }))
}

pub(super) fn read_event_texts(events_dir: &Path, filenames: &[String]) -> Vec<String> {
    let mut names = filenames.to_vec();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| std::fs::read_to_string(events_dir.join(name)).ok())
        .collect()
}

/// Build the plan-authoring continuation for response=approve_intent.
///
/// `mission_request` stays the public adapter, but the actual plan compile
/// still flows through unified_entry so the existing mission_plan gate,
/// compiler, and projection metadata remain authoritative.
pub(super) fn build_respond_plan_compile_args(
    args: &Value,
    directive: &DirectiveRef,
    request_id: &str,
) -> Value {
    let mut out = serde_json::Map::new();
    let board_task_id =
        nonblank(args.get("board_task_id")).unwrap_or_else(|| request_id.to_string());
    out.insert("approved_directive_id".into(), json!(directive.id.clone()));
    out.insert("directive_version".into(), json!(directive.version));
    out.insert("board_task_id".into(), json!(board_task_id));

    // The inner mission_plan compile only understands write_file; the
    // V3-preferred compat_write_file name and the legacy write_file alias
    // are both mission_request-local controls. Forward write_file=true to
    // the inner surface only when the caller opted into compat writes.
    // Per (compat-writer-policy ...) in .missiond/v3/missiond-blueprint.lisp.
    let compat_requested = match args.as_object() {
        Some(map) => compat_write_requested(map),
        None => false,
    };

    for key in [
        "compiler_mode",
        "persist",
        "target",
        "target_project",
        "dispatch_strategy",
        "parallelism",
        "objective",
        "requested_cwd",
        "flow_id",
        "overwrite_file",
        "topic",
        "project",
        "cwd",
        "review_gate_policy",
        "emit_review_question",
        "review_question_text",
        "review_question_id",
        "plan_acceptance",
        "plan_constraints",
    ] {
        if let Some(v) = args.get(key) {
            if !v.is_null() {
                out.insert(key.into(), v.clone());
            }
        }
    }
    if compat_requested {
        out.insert("write_file".into(), json!(true));
    }
    Value::Object(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoardTaskMaterialization {
    pub(super) board_task_id: String,
    pub(super) board_task_created: bool,
}

pub(super) fn board_task_materialization_to_json(m: &BoardTaskMaterialization) -> Value {
    json!({
        "board_task_id": m.board_task_id,
        "board_task_created": m.board_task_created,
        "source": "request-local review adapter",
    })
}

pub(super) async fn ensure_request_board_task(
    state: &AppState,
    args: &Value,
    request_id: &str,
    paths: &RequestPaths,
) -> Result<BoardTaskMaterialization> {
    if let Some(id) = nonblank(args.get("board_task_id")) {
        let task = state
            .store
            .get_board_task(&id)
            .await
            .map_err(|e| anyhow::anyhow!("DB error: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("board_task `{}` not found", id))?;
        return Ok(BoardTaskMaterialization {
            board_task_id: task.id.to_string(),
            board_task_created: false,
        });
    }

    let project = nonblank(args.get("project")).or_else(|| nonblank(args.get("target_project")));
    let input = CreateBoardTaskInput {
        title: format!("Mission request {} plan", request_id),
        description: Some(format!(
            "Hidden anchor for request-local plan materialized from {}.",
            path_json(&paths.plan)
        )),
        priority: Some("medium".into()),
        category: Some("dev".into()),
        project,
        hidden: Some(true),
        context_intent: Some("code".into()),
        ..Default::default()
    };
    let task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    Ok(BoardTaskMaterialization {
        board_task_id: task.id.to_string(),
        board_task_created: true,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlanMaterialization {
    pub(super) plan_ref: PlanRef,
    pub(super) board_task_id: String,
    pub(super) version: i32,
    pub(super) sexp_hash: String,
    pub(super) board_task_created: bool,
    pub(super) artifact_projection: Option<PlanArtifactProjection>,
    pub(super) artifact_projection_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlanArtifactProjection {
    pub(super) path: PathBuf,
    pub(super) sha256: String,
    pub(super) bytes: u64,
    pub(super) overwritten: bool,
}

pub(super) fn plan_materialization_to_json(m: &PlanMaterialization) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("plan_id".into(), json!(m.plan_ref.id));
    obj.insert("board_task_id".into(), json!(m.board_task_id));
    obj.insert("version".into(), json!(m.version));
    obj.insert("sexp_hash".into(), json!(m.sexp_hash));
    obj.insert("board_task_created".into(), json!(m.board_task_created));
    obj.insert("source".into(), json!("request-local plan.lisp"));
    if let Some(p) = m.artifact_projection.as_ref() {
        obj.insert(
            "artifact_projection".into(),
            json!({
                "path": path_json(&p.path),
                "sha256": p.sha256,
                "bytes": p.bytes,
                "overwritten": p.overwritten,
            }),
        );
    }
    if let Some(e) = m.artifact_projection_error.as_ref() {
        obj.insert("artifact_projection_error".into(), json!(e));
    }
    Value::Object(obj)
}

pub(super) fn enrich_materialized_plan_lisp(
    body: &str,
    plan_ref: &PlanRef,
    version: i32,
    board_task_id: &str,
) -> String {
    if body.contains(":plan_id") && body.contains(":version") && body.contains(":board_task_id") {
        return body.to_string();
    }

    let trimmed_len = body.trim_end().len();
    let trailing = &body[trimmed_len..];
    let mut core = body[..trimmed_len].to_string();
    if !core.ends_with(')') {
        return body.to_string();
    }

    core.pop();
    if !core.contains(":plan_id") {
        let _ = write!(core, "\n  :plan_id {}", lisp_string(&plan_ref.id));
    }
    if !core.contains(":version") {
        let _ = write!(core, "\n  :version {}", version);
    }
    if !core.contains(":board_task_id") {
        let _ = write!(core, "\n  :board_task_id {}", lisp_string(board_task_id));
    }
    core.push(')');
    core.push_str(trailing);
    core
}

pub(super) fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

pub(super) async fn materialize_request_plan(
    state: &AppState,
    args: &Value,
    request_id: &str,
    paths: &RequestPaths,
    plan_text: &str,
) -> Result<PlanMaterialization> {
    let mut anchor_args = args.clone();
    if nonblank(args.get("board_task_id")).is_none() {
        if let Some(board_task_id) = extract_lisp_keyword_string(plan_text, "board_task_id") {
            if let Some(obj) = anchor_args.as_object_mut() {
                obj.insert("board_task_id".into(), json!(board_task_id));
            }
        }
    }
    let anchor = ensure_request_board_task(state, &anchor_args, request_id, paths).await?;

    let existing = state
        .store
        .plan_list_by_task(&anchor.board_task_id)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    let version = existing.iter().map(|p| p.version).max().unwrap_or(0) + 1;
    let source_directive_id = extract_lisp_keyword_string(plan_text, "directive_id")
        .and_then(|id| uuid::Uuid::parse_str(&id).ok());
    let sexp_hash = sha256_hex(plan_text);
    let plan_id = state
        .store
        .plan_insert(
            &anchor.board_task_id,
            source_directive_id,
            version,
            plan_text,
            &sexp_hash,
            PlanStatus::Draft,
            None,
            Some("mission_request request-local plan.lisp"),
        )
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    let plan_ref = PlanRef {
        id: plan_id.to_string(),
    };
    let enriched_plan_text =
        enrich_materialized_plan_lisp(plan_text, &plan_ref, version, &anchor.board_task_id);
    let (artifact_projection, artifact_projection_error) = if enriched_plan_text != plan_text {
        match atomic_write_artifact(&paths.plan, &enriched_plan_text, true) {
            Ok(write) => (
                Some(PlanArtifactProjection {
                    path: write.path,
                    sha256: write.sha256,
                    bytes: write.bytes,
                    overwritten: write.overwritten,
                }),
                None,
            ),
            Err(e) => (None, Some(format!("{:#}", e))),
        }
    } else {
        (None, None)
    };

    Ok(PlanMaterialization {
        plan_ref,
        board_task_id: anchor.board_task_id,
        version,
        sexp_hash,
        board_task_created: anchor.board_task_created,
        artifact_projection,
        artifact_projection_error,
    })
}

/// Pure event-sequence allocator. The initial `request_received` event
/// occupies seq 1; review responses pick up at max(existing) + 1 so each
/// respond call lands a fresh, monotonically increasing event file.
pub(super) fn next_event_seq(existing_filenames: &[String]) -> u64 {
    let max = existing_filenames
        .iter()
        .filter_map(|n| parse_event_seq_from_filename(n))
        .max()
        .unwrap_or(0);
    max + 1
}

pub(super) fn parse_event_seq_from_filename(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".event.lisp")?;
    stem.parse::<u64>().ok()
}

pub(super) fn event_path_for_seq(events_dir: &Path, seq: u64) -> PathBuf {
    events_dir.join(format!("{:06}.event.lisp", seq))
}

pub(super) fn list_event_filenames(events_dir: &Path) -> Vec<String> {
    let read = match std::fs::read_dir(events_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    read.filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RespondOutcome {
    Recorded,
    Dispatched,
    Blocked,
}

impl RespondOutcome {
    pub(super) fn wire(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Dispatched => "dispatched",
            Self::Blocked => "blocked",
        }
    }

    fn event_kind(self) -> &'static str {
        match self {
            Self::Recorded => "review_response_recorded",
            Self::Dispatched => "review_response_dispatched",
            Self::Blocked => "review_response_blocked",
        }
    }
}

pub(super) struct ReviewEventArgs<'a> {
    pub(super) request_id: &'a str,
    pub(super) seq: u64,
    pub(super) decision: RespondDecision,
    pub(super) outcome: RespondOutcome,
    pub(super) note: Option<&'a str>,
    pub(super) directive_ref: Option<&'a DirectiveRef>,
    pub(super) plan_ref: Option<&'a PlanRef>,
    pub(super) execute: bool,
    pub(super) inner_action: Option<&'a str>,
    pub(super) blocked_reason: Option<&'a str>,
    pub(super) created_at: &'a str,
}

pub(super) fn build_review_event_lisp(args: &ReviewEventArgs<'_>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, ";; MissionD review-response event.");
    let _ = writeln!(out, ";; Schema: {}", EVENT_SCHEMA);
    let event_id = format!("evt-{}-{:06}", args.request_id, args.seq);
    let _ = writeln!(out, "(lifecycle-event {}", lisp_string(&event_id));
    let _ = writeln!(out, "  :schema {}", lisp_string(EVENT_SCHEMA));
    let _ = writeln!(out, "  :seq {}", args.seq);
    let _ = writeln!(out, "  :event_id {}", lisp_string(&event_id));
    let _ = writeln!(out, "  :request_id {}", lisp_string(args.request_id));
    let _ = writeln!(out, "  :kind :{}", args.outcome.event_kind());
    let _ = writeln!(
        out,
        "  :actor (:role :user :id \"mission_request.respond\")"
    );
    let _ = writeln!(out, "  :time {}", lisp_string(args.created_at));
    let _ = writeln!(out, "  :payload");
    let _ = writeln!(out, "    (:decision :{}", args.decision.wire());
    let _ = writeln!(out, "     :outcome :{}", args.outcome.wire());
    if let Some(note) = args.note {
        let _ = writeln!(out, "     :note {}", lisp_string(note));
    }
    if let Some(d) = args.directive_ref {
        let _ = writeln!(out, "     :directive_id {}", lisp_string(&d.id));
        let _ = writeln!(out, "     :directive_version {}", d.version);
    }
    if let Some(p) = args.plan_ref {
        let _ = writeln!(out, "     :plan_id {}", lisp_string(&p.id));
    }
    let _ = writeln!(
        out,
        "     :execute {}",
        if args.execute { "true" } else { "false" }
    );
    if let Some(inner) = args.inner_action {
        let _ = writeln!(out, "     :inner_action {}", lisp_string(inner));
    }
    if let Some(reason) = args.blocked_reason {
        let _ = writeln!(out, "     :blocked_reason {}", lisp_string(reason));
    }
    let _ = writeln!(out, "    )");
    let _ = writeln!(
        out,
        "  :idempotency_key {})",
        lisp_string(&format!(
            "{}/{}/{:06}",
            args.request_id,
            args.outcome.event_kind(),
            args.seq
        ))
    );
    out
}

pub(super) fn next_action_for(decision: RespondDecision, outcome: RespondOutcome) -> &'static str {
    match (decision, outcome) {
        (RespondDecision::ApproveIntent, RespondOutcome::Dispatched) => {
            "directive approved and plan.lisp projection requested; review the returned plan review_packet"
        }
        (RespondDecision::ApprovePlan, RespondOutcome::Dispatched) => {
            "plan approved; call mission_request respond with response=execute_plan + execute=true to dispatch the plan"
        }
        (RespondDecision::ExecutePlan, RespondOutcome::Dispatched) => {
            "plan execute requested; observe execution status through mission_request status and task receipts"
        }
        (RespondDecision::RejectIntent, RespondOutcome::Recorded) => {
            "rejection recorded; revise the message and call mission_request start again"
        }
        (RespondDecision::RejectPlan, RespondOutcome::Recorded) => {
            "rejection recorded; revise the plan source and call mission_request advance or start again"
        }
        (RespondDecision::AskQuestion, RespondOutcome::Recorded) => {
            "question recorded; wait for follow-up answer, then call mission_request respond again"
        }
        (_, RespondOutcome::Blocked) => {
            "supply the missing reference (or required flag) and re-call mission_request respond"
        }
        _ => "review_packet describes the next legal continuation",
    }
}

pub(super) async fn action_respond(state: &AppState, args: &Value) -> Result<ToolResult> {
    let request_id_raw = match nonblank(args.get("request_id")) {
        Some(id) => id,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "respond requires `request_id`")
                    .with_suggestion(
                        "pass the request_id returned by mission_request(action=start)",
                    ),
            ))
        }
    };
    let request_id = sanitize_request_id(&request_id_raw);

    let decision = match parse_respond_decision(args) {
        Ok(d) => d,
        Err(e) => return Ok(ToolResult::structured_error(e.into_tool_error())),
    };

    let root = match resolve_request_project_root(state, args).await {
        Ok(root) => root,
        Err(reason) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, reason)
                    .with_suggestion("pass project, absolute cwd, or target_project"),
            ))
        }
    };
    let paths = request_paths_for(&root, &request_id);

    let request_text = std::fs::read_to_string(&paths.request).ok();
    let mode = match request_text.as_deref() {
        Some(text) => extract_mode_from_request_lisp(text),
        None => RequestMode::HumanInteractive,
    };
    let request_exists = request_text.is_some();

    let intent_text = std::fs::read_to_string(&paths.intent_alignment).ok();
    let plan_text = std::fs::read_to_string(&paths.plan).ok();
    let event_filenames = list_event_filenames(&paths.events_dir);
    let event_texts = read_event_texts(&paths.events_dir, &event_filenames);
    let directive_ref = if decision.requires_directive_ref() {
        resolve_directive_ref(args, intent_text.as_deref())
    } else {
        None
    };
    let mut plan_ref = if decision.requires_plan_ref() {
        resolve_plan_ref(args, plan_text.as_deref(), &event_texts)
    } else {
        None
    };

    let note = nonblank(args.get("note"));
    let overwrite = args
        .get("overwrite_file")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let execute_flag_explicit = args
        .get("execute")
        .and_then(|v| v.as_bool())
        .or_else(|| args.get("execute_after_approval").and_then(|v| v.as_bool()));

    // Pure decision routing — pick outcome / inner action / blocked reason
    // before any IO so unit tests can pin the routing without AppState.
    let mut outcome = if decision.record_only() {
        RespondOutcome::Recorded
    } else {
        RespondOutcome::Dispatched
    };
    let mut blocked_reason: Option<String> = None;
    let mut inner_action: Option<&'static str> = None;
    let mut effective_execute = false;
    let mut board_task_materialization: Option<BoardTaskMaterialization> = None;
    let mut plan_materialization: Option<PlanMaterialization> = None;

    if !request_exists {
        outcome = RespondOutcome::Blocked;
        blocked_reason = Some(format!(
            "request.lisp missing at {}; call mission_request(action=start) first",
            path_json(&paths.request)
        ));
    }

    if outcome != RespondOutcome::Blocked
        && decision.requires_directive_ref()
        && directive_ref.is_none()
    {
        outcome = RespondOutcome::Blocked;
        blocked_reason = Some(
            "directive ref missing; pass approved_directive_id (or directive_id) + directive_version, or wait for intent-alignment.lisp to carry a persisted ref".into(),
        );
    }

    if outcome != RespondOutcome::Blocked
        && matches!(decision, RespondDecision::ApprovePlan)
        && plan_ref.is_none()
    {
        match plan_text.as_deref() {
            Some(text) => {
                match materialize_request_plan(state, args, &request_id, &paths, text).await {
                    Ok(materialized) => {
                        plan_ref = Some(materialized.plan_ref.clone());
                        plan_materialization = Some(materialized);
                    }
                    Err(e) => {
                        outcome = RespondOutcome::Blocked;
                        blocked_reason = Some(format!(
                            "failed to materialize request-local plan.lisp: {:#}",
                            e
                        ));
                    }
                }
            }
            None => {}
        }
    }

    if outcome != RespondOutcome::Blocked && decision.requires_plan_ref() && plan_ref.is_none() {
        outcome = RespondOutcome::Blocked;
        blocked_reason = Some(
            "plan ref missing; pass approved_plan_id (or plan_id), approve request-local plan.lisp first, or wait for a prior approve_plan review event to carry a persisted ref".into(),
        );
    }

    if outcome != RespondOutcome::Blocked && matches!(decision, RespondDecision::ExecutePlan) {
        match execute_flag_explicit {
            Some(false) => {
                outcome = RespondOutcome::Blocked;
                blocked_reason = Some(
                    "execute_plan requires execute=true (or omit `execute` so response=execute_plan implies it)".into(),
                );
            }
            _ => {}
        }
    }

    let allocated_seq = next_event_seq(&event_filenames);
    let event_path = event_path_for_seq(&paths.events_dir, allocated_seq);
    let created_at = now_rfc3339();

    let mut inner_payload: Option<Value> = None;
    let mut inner_is_error = false;
    let mut projection_payload: Option<Value> = None;

    if outcome == RespondOutcome::Dispatched {
        match decision {
            RespondDecision::ApproveIntent => {
                let d = directive_ref.as_ref().expect("ref enforced above");
                let inner_args = json!({
                    "action": "approve",
                    "directive_id": d.id,
                    "version": d.version,
                });
                let inner =
                    super::super::directive::handle(state, "mission_directive", inner_args).await?;
                let approve_is_error = inner.is_error.unwrap_or(false);
                let approve_payload = tool_result_payload(&inner);
                let mut combined = json!({
                    "approval": approve_payload,
                });

                if approve_is_error {
                    inner_is_error = true;
                    inner_action = Some("mission_directive::approve");
                } else {
                    match ensure_request_board_task(state, args, &request_id, &paths).await {
                        Ok(anchor) => {
                            let mut plan_args =
                                build_respond_plan_compile_args(args, d, &request_id);
                            if let Some(obj) = plan_args.as_object_mut() {
                                obj.insert(
                                    "board_task_id".into(),
                                    json!(anchor.board_task_id.clone()),
                                );
                            }
                            if let Some(obj) = combined.as_object_mut() {
                                obj.insert(
                                    "plan_anchor".into(),
                                    board_task_materialization_to_json(&anchor),
                                );
                            }
                            board_task_materialization = Some(anchor);

                            let plan_inner =
                                super::super::unified_entry::run_pipeline(state, plan_args).await?;
                            let plan_is_error = plan_inner.is_error.unwrap_or(false);
                            let projection =
                                run_projection(&plan_inner, Some(&paths), overwrite, true);
                            let projection_json = projection_to_json(&projection);
                            let projection_failed = projection.status != ProjectionStatus::Written;

                            if let Some(obj) = combined.as_object_mut() {
                                obj.insert("plan_compile".into(), tool_result_payload(&plan_inner));
                                obj.insert("projection".into(), projection_json.clone());
                            }
                            projection_payload = Some(projection_json);
                            inner_is_error = plan_is_error || projection_failed;
                            if projection_failed && blocked_reason.is_none() {
                                blocked_reason = Some(format!(
                                    "plan.lisp projection did not complete (status={})",
                                    projection.status.wire()
                                ));
                            }
                            inner_action =
                                Some("mission_directive::approve+unified_entry::plan_compile");
                        }
                        Err(e) => {
                            inner_is_error = true;
                            blocked_reason = Some(format!(
                                "failed to prepare request-local BoardTask anchor: {:#}",
                                e
                            ));
                            if let Some(obj) = combined.as_object_mut() {
                                obj.insert(
                                    "plan_anchor".into(),
                                    json!({
                                        "status": "error",
                                        "reason": format!("{:#}", e),
                                    }),
                                );
                            }
                            inner_action = Some("mission_directive::approve+board_task_anchor");
                        }
                    }
                }
                inner_payload = Some(combined);
            }
            RespondDecision::ApprovePlan => {
                let p = plan_ref.as_ref().expect("ref enforced above");
                let inner_args = json!({
                    "action": "approve",
                    "plan_id": p.id,
                });
                let inner = super::super::plan::handle(state, "mission_plan", inner_args).await?;
                inner_is_error = inner.is_error.unwrap_or(false);
                inner_payload = Some(tool_result_payload(&inner));
                inner_action = Some("mission_plan::approve");
            }
            RespondDecision::ExecutePlan => {
                let p = plan_ref.as_ref().expect("ref enforced above");
                let mut pipeline_args = serde_json::Map::new();
                pipeline_args.insert("approved_plan_id".into(), json!(p.id));
                pipeline_args.insert("execute".into(), json!(true));
                for key in [
                    "target",
                    "execute_mode",
                    "scheduler_mode",
                    "dispatch_strategy",
                    "parallelism",
                    "objective",
                    "flow_id",
                    "dry_run",
                    "project",
                    "cwd",
                    "target_project",
                    "review_question_id",
                ] {
                    if let Some(v) = args.get(key) {
                        if !v.is_null() {
                            pipeline_args.insert(key.into(), v.clone());
                        }
                    }
                }
                let inner =
                    super::super::unified_entry::run_pipeline(state, Value::Object(pipeline_args))
                        .await?;
                inner_is_error = inner.is_error.unwrap_or(false);
                inner_payload = Some(tool_result_payload(&inner));
                if !inner_is_error {
                    effective_execute = true;
                }
                inner_action = Some("unified_entry::plan_execute");
            }
            _ => {}
        }
    }

    if inner_is_error {
        outcome = RespondOutcome::Blocked;
        if blocked_reason.is_none() {
            blocked_reason =
                Some("inner approval/execute surface returned a structured error".into());
        }
    }

    let event_body = build_review_event_lisp(&ReviewEventArgs {
        request_id: &request_id,
        seq: allocated_seq,
        decision,
        outcome,
        note: note.as_deref(),
        directive_ref: directive_ref.as_ref(),
        plan_ref: plan_ref.as_ref(),
        execute: effective_execute,
        inner_action,
        blocked_reason: blocked_reason.as_deref(),
        created_at: &created_at,
    });
    let event_write_outcome = atomic_write_artifact(&event_path, &event_body, false);
    let event_write = match event_write_outcome {
        Ok(o) => o,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "failed to append review event {}: {:#}",
                        event_path.display(),
                        e
                    ),
                )
                .with_suggestion("ensure the request_id and project root are correct"),
            ))
        }
    };

    let existence = read_artifact_existence(&paths);
    let mut updated_event_texts = event_texts.clone();
    updated_event_texts.push(event_body.clone());
    let review_packet = derive_review_packet(
        &ReviewPacketInputs {
            mode,
            paths: &paths,
            existence,
            projection_target: None,
            fallback_preview: None,
            execute_requested: effective_execute,
            review_checkpoint: latest_review_event_checkpoint(&updated_event_texts),
        },
        |p| std::fs::read_to_string(p).ok(),
    );

    let next_action = next_action_for(decision, outcome);

    let mut respond_result = serde_json::Map::new();
    respond_result.insert("decision".into(), json!(decision.wire()));
    respond_result.insert("outcome".into(), json!(outcome.wire()));
    respond_result.insert("event_path".into(), json!(path_json(&event_write.path)));
    respond_result.insert("event_seq".into(), json!(allocated_seq));
    respond_result.insert("event_sha256".into(), json!(event_write.sha256));
    respond_result.insert("event_bytes".into(), json!(event_write.bytes));
    respond_result.insert("execute".into(), json!(effective_execute));
    respond_result.insert("next_action".into(), json!(next_action));
    if let Some(d) = directive_ref.as_ref() {
        respond_result.insert("directive_id".into(), json!(d.id));
        respond_result.insert("directive_version".into(), json!(d.version));
    }
    if let Some(p) = plan_ref.as_ref() {
        respond_result.insert("plan_id".into(), json!(p.id));
    }
    if let Some(b) = board_task_materialization.as_ref() {
        respond_result.insert(
            "board_task_materialized".into(),
            json!(b.board_task_created),
        );
        respond_result.insert(
            "board_task_materialization".into(),
            board_task_materialization_to_json(b),
        );
    }
    if let Some(m) = plan_materialization.as_ref() {
        respond_result.insert("plan_materialized".into(), json!(true));
        respond_result.insert(
            "plan_materialization".into(),
            plan_materialization_to_json(m),
        );
    }
    if let Some(inner) = inner_action {
        respond_result.insert("inner_action".into(), json!(inner));
    }
    if let Some(reason) = blocked_reason.as_ref() {
        respond_result.insert("blocked_reason".into(), json!(reason));
    }
    if let Some(n) = note.as_ref() {
        respond_result.insert("note".into(), json!(n));
    }

    let mut response = json!({
        "status": match outcome {
            RespondOutcome::Blocked => "blocked",
            _ => "ok",
        },
        "action": "respond",
        "mode": mode.wire(),
        "request_id": request_id,
        "request_path": path_json(&paths.request),
        "artifact_paths": build_artifact_paths_json(&paths),
        "artifact_exists": build_artifact_existence(&paths),
        "respond_result": Value::Object(respond_result),
        "review_packet": review_packet,
        "next_action": next_action,
        "v3_contract": {
            "blueprint": ".missiond/v3/missiond-blueprint.lisp",
            "surface": "mission_request",
            "feature": "review-response"
        }
    });
    if let Some(payload) = inner_payload {
        if let Some(obj) = response.as_object_mut() {
            obj.insert("pipeline_result".into(), payload);
        }
    }
    if let Some(projection) = projection_payload {
        if let Some(obj) = response.as_object_mut() {
            obj.insert("projection".into(), projection);
        }
    }
    if let Some(m) = plan_materialization.as_ref() {
        if let Some(obj) = response.as_object_mut() {
            obj.insert(
                "plan_materialization".into(),
                plan_materialization_to_json(m),
            );
        }
    }
    if let Some(b) = board_task_materialization.as_ref() {
        if let Some(obj) = response.as_object_mut() {
            obj.insert(
                "board_task_materialization".into(),
                board_task_materialization_to_json(b),
            );
        }
    }

    let mut out = ToolResult::json_pretty(&response);
    if outcome == RespondOutcome::Blocked {
        out.is_error = Some(true);
    }
    Ok(out)
}
