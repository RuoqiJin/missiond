//! mission_request — v3 unified request entry.
//!
//! Lisp authority:
//!   - .missiond/v3/missiond-blueprint.lisp :: mission_request surface
//!   - .missiond/v3/missiond-blueprint.lisp :: unified-entry state-machine
//!   - .missiond/v3/missiond-blueprint.lisp :: artifact mission-request /
//!     intent-alignment / plan / lifecycle-event
//!
//! v0 is intentionally conservative:
//!   - file-first request.lisp + initial lifecycle event;
//!   - request-local Lisp projections (intent-alignment.lisp / plan.lisp)
//!     mirrored from stable inner directive/plan compile payloads;
//!   - no DB schema migration;
//!   - no auto-approval of intent or plan;
//!   - no direct workstation dispatch;
//!   - all actual directive/plan work is delegated to the existing
//!     unified_entry helper, which itself composes mission_directive and
//!     mission_plan.

use anyhow::Result;
use missiond_core::types::{CreateBoardTaskInput, PlanStatus};
use missiond_core::util::safe_byte_truncate;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::handlers::knowledge::file_artifacts::atomic_write_artifact;
use crate::state::AppState;

mod request_artifacts;

#[cfg(test)]
use missiond_mcp::tools::ToolContent;
use request_artifacts::{
    build_artifact_existence, build_artifact_paths_json, build_event_lisp, build_request_lisp,
    extract_projected_sexp, nonblank, now_rfc3339, parse_mode, path_json, projection_to_json,
    request_id_from_args, request_paths_for, resolve_request_project_root, run_projection,
    sanitize_request_id, tool_result_payload, ProjectionOutcome, ProjectionStatus, RequestDoc,
    RequestMode, RequestPaths,
};
#[cfg(test)]
use request_artifacts::{
    build_artifact_existence_with, classify_projection_target, extract_pipeline_meta,
    plan_projection, PipelineMeta, ProjectionPlan, ProjectionTarget,
};

const REQUEST_SCHEMA: &str = "missiond.request.v1";
const EVENT_SCHEMA: &str = "missiond.lifecycle-event.v1";

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "mission_request requires `action`",
                )
                .with_suggestion("actions: start|advance|status|respond"),
            ))
        }
    };

    match action {
        "start" => action_start(state, &args).await,
        "advance" => action_advance(state, &args).await,
        "status" => action_status(state, &args).await,
        "respond" => action_respond(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_request action `{}`", other),
            )
            .with_suggestion("valid: start|advance|status|respond"),
        )),
    }
}

async fn action_start(state: &AppState, args: &Value) -> Result<ToolResult> {
    let message = match nonblank(args.get("message")) {
        Some(m) => m,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "start requires `message`")
                    .with_suggestion("pass the user need / external request body as message"),
            ))
        }
    };

    let request_id = request_id_from_args(args);
    let mode = parse_mode(args.get("mode").and_then(|v| v.as_str()));
    let write_request_file = args
        .get("write_request_file")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let overwrite = args
        .get("overwrite_file")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let created_at = now_rfc3339();

    let mut file_payload = json!({
        "request_id": request_id.clone(),
        "request_written": false,
        "event_written": false,
    });

    // Resolve project root once. write_request_file=true makes resolution
    // mandatory (we cannot stub the request.lisp path); preview-only routing
    // (write_request_file=false) tolerates missing project context and just
    // surfaces a `skipped_no_project_root` projection status downstream.
    let project_root_result = resolve_request_project_root(state, args).await;
    let mut request_paths: Option<RequestPaths> = None;

    if write_request_file {
        let root = match project_root_result {
            Ok(root) => root,
            Err(reason) => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(error_codes::INVALID_PARAM, reason).with_suggestion(
                        "pass project, absolute cwd, or target_project; mission_request refuses process-cwd fallback",
                    ),
                ))
            }
        };

        let paths = request_paths_for(&root, &request_id);
        let body = build_request_lisp(&RequestDoc {
            request_id: &request_id,
            mode,
            source: nonblank(args.get("source"))
                .as_deref()
                .unwrap_or("user_request"),
            objective: &message,
            created_at: &created_at,
            paths: &paths,
        });
        let request_write = match atomic_write_artifact(&paths.request, &body, overwrite) {
            Ok(outcome) => outcome,
            Err(e) => return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("failed to write {}: {:#}", paths.request.display(), e),
                )
                .with_suggestion(
                    "use overwrite_file=true only when replacing the same request intentionally",
                ),
            )),
        };

        let event_body = build_event_lisp(&request_id, &created_at, "request_received", &message);
        let event_write = match atomic_write_artifact(&paths.initial_event, &event_body, overwrite)
        {
            Ok(outcome) => outcome,
            Err(e) => return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("failed to write {}: {:#}", paths.initial_event.display(), e),
                )
                .with_suggestion(
                    "use overwrite_file=true only when replacing the same request intentionally",
                ),
            )),
        };

        file_payload = json!({
            "request_id": request_id.clone(),
            "request_written": true,
            "request_path": path_json(&paths.request),
            "request_sha256": request_write.sha256,
            "request_bytes": request_write.bytes,
            "event_written": true,
            "initial_event_path": path_json(&paths.initial_event),
            "initial_event_sha256": event_write.sha256,
            "initial_event_bytes": event_write.bytes,
            "artifact_paths": build_artifact_paths_json(&paths),
        });
        request_paths = Some(paths);
    } else if let Ok(root) = project_root_result.as_ref() {
        request_paths = Some(request_paths_for(root, &request_id));
    }

    let mut pipeline_args = args.clone();
    normalize_start_args(&mut pipeline_args, &request_id);
    let inner = super::unified_entry::run_pipeline(state, pipeline_args).await?;

    let projection = run_projection(&inner, request_paths.as_ref(), overwrite, true);
    let execute_requested = parse_execute_requested(args);
    Ok(wrap_pipeline_result(
        "start",
        mode,
        file_payload,
        projection,
        request_paths.as_ref(),
        execute_requested,
        inner,
    ))
}

async fn action_advance(state: &AppState, args: &Value) -> Result<ToolResult> {
    let request_id_raw = nonblank(args.get("request_id"));
    let sanitized_request_id = request_id_raw.as_deref().map(sanitize_request_id);
    let mode = parse_mode(args.get("mode").and_then(|v| v.as_str()));
    let overwrite = args
        .get("overwrite_file")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let project_root_result = resolve_request_project_root(state, args).await;
    let request_paths = match (
        sanitized_request_id.as_deref(),
        project_root_result.as_ref(),
    ) {
        (Some(id), Ok(root)) => Some(request_paths_for(root, id)),
        _ => None,
    };

    let inner = super::unified_entry::run_pipeline(state, args.clone()).await?;

    let request_id_present = sanitized_request_id.is_some();
    let projection = run_projection(
        &inner,
        request_paths.as_ref(),
        overwrite,
        request_id_present,
    );
    let execute_requested = parse_execute_requested(args);

    let file_payload = json!({
        "request_id": sanitized_request_id,
        "request_written": false,
        "event_written": false,
    });
    Ok(wrap_pipeline_result(
        "advance",
        mode,
        file_payload,
        projection,
        request_paths.as_ref(),
        execute_requested,
        inner,
    ))
}

async fn action_status(state: &AppState, args: &Value) -> Result<ToolResult> {
    let request_id = match nonblank(args.get("request_id")) {
        Some(id) => sanitize_request_id(&id),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "status requires `request_id`")
                    .with_suggestion(
                        "pass the request_id returned by mission_request(action=start)",
                    ),
            ))
        }
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
    let text = match std::fs::read_to_string(&paths.request) {
        Ok(text) => text,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("failed to read {}: {}", paths.request.display(), e),
                )
                .with_suggestion("check request_id and project/cwd resolution"),
            ))
        }
    };

    let mode = extract_mode_from_request_lisp(&text);
    let existence = read_artifact_existence(&paths);
    let event_filenames = list_event_filenames(&paths.events_dir);
    let event_texts = read_event_texts(&paths.events_dir, &event_filenames);
    let inputs = ReviewPacketInputs {
        mode,
        paths: &paths,
        existence,
        projection_target: None,
        fallback_preview: None,
        execute_requested: false,
        review_checkpoint: latest_review_event_checkpoint(&event_texts),
    };
    let review_packet = derive_review_packet(&inputs, |p| std::fs::read_to_string(p).ok());

    Ok(ToolResult::json_pretty(&json!({
        "status": "ok",
        "action": "status",
        "mode": mode.wire(),
        "request_id": request_id,
        "request_path": path_json(&paths.request),
        "request_lisp": text,
        "artifact_paths": build_artifact_paths_json(&paths),
        "artifact_exists": build_artifact_existence(&paths),
        "review_packet": review_packet,
    })))
}

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
enum RespondDecision {
    ApproveIntent,
    RejectIntent,
    AskQuestion,
    ApprovePlan,
    RejectPlan,
    ExecutePlan,
}

impl RespondDecision {
    fn wire(self) -> &'static str {
        match self {
            Self::ApproveIntent => "approve_intent",
            Self::RejectIntent => "reject_intent",
            Self::AskQuestion => "ask_question",
            Self::ApprovePlan => "approve_plan",
            Self::RejectPlan => "reject_plan",
            Self::ExecutePlan => "execute_plan",
        }
    }

    fn requires_directive_ref(self) -> bool {
        matches!(self, Self::ApproveIntent | Self::RejectIntent)
    }

    fn requires_plan_ref(self) -> bool {
        matches!(
            self,
            Self::ApprovePlan | Self::RejectPlan | Self::ExecutePlan
        )
    }

    /// Record-only routes never mutate directive/plan approval state and
    /// only persist a request-local review event so the user decision
    /// remains auditable.
    fn record_only(self) -> bool {
        matches!(
            self,
            Self::RejectIntent | Self::RejectPlan | Self::AskQuestion
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RespondParseError {
    Missing,
    Unknown(String),
}

impl RespondParseError {
    fn into_tool_error(self) -> ToolError {
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
fn parse_respond_decision(args: &Value) -> std::result::Result<RespondDecision, RespondParseError> {
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
struct DirectiveRef {
    id: String,
    version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanRef {
    id: String,
}

/// Best-effort scan of a Lisp artifact for `:<key> "<uuid>"`. Pure helper —
/// no IO, no regex crate. Picks the first occurrence so callers keep the
/// canonical persisted ref ahead of any later debug noise.
fn extract_lisp_keyword_string(text: &str, key: &str) -> Option<String> {
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

fn extract_lisp_keyword_int(text: &str, key: &str) -> Option<i32> {
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

fn is_uuid_shaped(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

fn resolve_directive_ref(
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

fn extract_directive_ref_from_artifact(text: &str) -> Option<DirectiveRef> {
    let id = match extract_lisp_keyword_string(text, "directive_id") {
        Some(id) => id,
        None => extract_lisp_keyword_string(text, "id").filter(|id| is_uuid_shaped(id))?,
    };
    let version = extract_lisp_keyword_int(text, "directive_version")
        .or_else(|| extract_lisp_keyword_int(text, "version"))?;
    Some(DirectiveRef { id, version })
}

fn resolve_plan_ref(
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

fn extract_plan_ref_from_artifact(text: &str) -> Option<PlanRef> {
    if let Some(id) = extract_lisp_keyword_string(text, "plan_id") {
        return Some(PlanRef { id });
    }
    // Request-local plan.lisp may contain nested node ids such as
    // `(:id "root" ...)`; never treat those as persisted plan refs.
    extract_lisp_keyword_string(text, "id")
        .filter(|id| is_uuid_shaped(id))
        .map(|id| PlanRef { id })
}

fn extract_latest_plan_ref_from_events(event_texts: &[String]) -> Option<PlanRef> {
    event_texts
        .iter()
        .rev()
        .find_map(|text| extract_lisp_keyword_string(text, "plan_id").map(|id| PlanRef { id }))
}

fn read_event_texts(events_dir: &Path, filenames: &[String]) -> Vec<String> {
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
fn build_respond_plan_compile_args(
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
struct BoardTaskMaterialization {
    board_task_id: String,
    board_task_created: bool,
}

fn board_task_materialization_to_json(m: &BoardTaskMaterialization) -> Value {
    json!({
        "board_task_id": m.board_task_id,
        "board_task_created": m.board_task_created,
        "source": "request-local review adapter",
    })
}

async fn ensure_request_board_task(
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
struct PlanMaterialization {
    plan_ref: PlanRef,
    board_task_id: String,
    version: i32,
    sexp_hash: String,
    board_task_created: bool,
    artifact_projection: Option<PlanArtifactProjection>,
    artifact_projection_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanArtifactProjection {
    path: PathBuf,
    sha256: String,
    bytes: u64,
    overwritten: bool,
}

fn plan_materialization_to_json(m: &PlanMaterialization) -> Value {
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

fn enrich_materialized_plan_lisp(
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

fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

async fn materialize_request_plan(
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
fn next_event_seq(existing_filenames: &[String]) -> u64 {
    let max = existing_filenames
        .iter()
        .filter_map(|n| parse_event_seq_from_filename(n))
        .max()
        .unwrap_or(0);
    max + 1
}

fn parse_event_seq_from_filename(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".event.lisp")?;
    stem.parse::<u64>().ok()
}

fn event_path_for_seq(events_dir: &Path, seq: u64) -> PathBuf {
    events_dir.join(format!("{:06}.event.lisp", seq))
}

fn list_event_filenames(events_dir: &Path) -> Vec<String> {
    let read = match std::fs::read_dir(events_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    read.filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RespondOutcome {
    Recorded,
    Dispatched,
    Blocked,
}

impl RespondOutcome {
    fn wire(self) -> &'static str {
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

struct ReviewEventArgs<'a> {
    request_id: &'a str,
    seq: u64,
    decision: RespondDecision,
    outcome: RespondOutcome,
    note: Option<&'a str>,
    directive_ref: Option<&'a DirectiveRef>,
    plan_ref: Option<&'a PlanRef>,
    execute: bool,
    inner_action: Option<&'a str>,
    blocked_reason: Option<&'a str>,
    created_at: &'a str,
}

fn build_review_event_lisp(args: &ReviewEventArgs<'_>) -> String {
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

fn next_action_for(decision: RespondDecision, outcome: RespondOutcome) -> &'static str {
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

async fn action_respond(state: &AppState, args: &Value) -> Result<ToolResult> {
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
                    super::directive::handle(state, "mission_directive", inner_args).await?;
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
                                super::unified_entry::run_pipeline(state, plan_args).await?;
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
                let inner = super::plan::handle(state, "mission_plan", inner_args).await?;
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
                    super::unified_entry::run_pipeline(state, Value::Object(pipeline_args)).await?;
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

fn normalize_start_args(args: &mut Value, request_id: &str) {
    if let Some(map) = args.as_object_mut() {
        map.insert("action".into(), json!("start-forwarded"));
        map.entry("source").or_insert_with(|| json!("user_request"));
        map.entry("topic").or_insert_with(|| json!(request_id));
    }
    apply_compat_write_file_policy(args);
}

/// Derives `compat_write_requested = compat_write_file == true || write_file == true`
/// from the caller args, then rewrites the args forwarded to the inner
/// directive / plan compile so:
///   - both `compat_write_file` and `write_file` keys are removed (they are
///     mission_request-local controls; the inner surfaces only know about
///     `write_file`),
///   - `write_file: true` is re-injected only when compat was explicitly
///     requested.
///
/// Default mission_request flow therefore never writes the legacy
/// .missiond/alignment/<topic>/ or .missiond/plans/<plan_id>/ projections.
/// Per (compat-writer-policy ...) in .missiond/v3/missiond-blueprint.lisp.
fn apply_compat_write_file_policy(args: &mut Value) {
    let map = match args.as_object_mut() {
        Some(m) => m,
        None => return,
    };
    let compat_requested = compat_write_requested(map);
    map.remove("compat_write_file");
    map.remove("write_file");
    if compat_requested {
        map.insert("write_file".into(), json!(true));
    }
}

fn compat_write_requested(map: &serde_json::Map<String, Value>) -> bool {
    let compat = map
        .get("compat_write_file")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let legacy = map
        .get("write_file")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    compat || legacy
}

fn wrap_pipeline_result(
    request_action: &str,
    mode: RequestMode,
    request_artifacts: Value,
    projection: ProjectionOutcome,
    request_paths: Option<&RequestPaths>,
    execute_requested: bool,
    inner: ToolResult,
) -> ToolResult {
    let inner_is_error = inner.is_error.unwrap_or(false);
    let inner_payload = tool_result_payload(&inner);
    let fallback_preview = extract_projected_sexp(&inner_payload).map(|(body, _)| body);
    let review_packet = request_paths.map(|paths| {
        let existence = read_artifact_existence(paths);
        let inputs = ReviewPacketInputs {
            mode,
            paths,
            existence,
            projection_target: projection.target,
            fallback_preview: fallback_preview.as_deref(),
            execute_requested,
            review_checkpoint: None,
        };
        derive_review_packet(&inputs, |p| std::fs::read_to_string(p).ok())
    });
    let mut response = json!({
        "status": if inner_is_error { "pipeline_error" } else { "ok" },
        "action": request_action,
        "mode": mode.wire(),
        "request_artifacts": request_artifacts,
        "projection": projection_to_json(&projection),
        "pipeline": inner_payload,
        "v3_contract": {
            "blueprint": ".missiond/v3/missiond-blueprint.lisp",
            "surface": "mission_request",
            "review_gates": if mode == RequestMode::HumanInteractive {
                json!(["intent-review-gate", "plan-review-gate"])
            } else {
                json!(["trusted-agent-policy", "risk-gate", "scoped-write-gate"])
            }
        },
        "next_step": if mode == RequestMode::HumanInteractive {
            "review the returned intent/plan artifact, then answer through mission_request(action=respond)"
        } else {
            "trusted-agent mode may continue with mission_request(action=advance) only when policy gates allow it"
        }
    });
    if let Some(obj) = response.as_object_mut() {
        obj.insert("inner_is_error".into(), json!(inner_is_error));
        if let Some(paths) = request_paths {
            obj.insert("artifact_paths".into(), build_artifact_paths_json(paths));
            obj.insert("artifact_exists".into(), build_artifact_existence(paths));
        }
        if let Some(packet) = review_packet {
            obj.insert("review_packet".into(), packet);
        }
    }
    let mut out = ToolResult::json_pretty(&response);
    if inner_is_error {
        out.is_error = Some(true);
    }
    out
}

// ───────────────────────────────────────────────────────────────────────
// Review packet — V3 unified-entry projection. Pure derivation from
// request-local artifact existence + latest projection target/preview.
// Never approves intent or plan, never dispatches workstation work.
// ───────────────────────────────────────────────────────────────────────

const REVIEW_PREVIEW_MAX_BYTES: usize = 480;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ArtifactExistence {
    request: bool,
    intent_alignment: bool,
    plan: bool,
}

fn read_artifact_existence(paths: &RequestPaths) -> ArtifactExistence {
    ArtifactExistence {
        request: paths.request.exists(),
        intent_alignment: paths.intent_alignment.exists(),
        plan: paths.plan.exists(),
    }
}

struct ReviewPacketInputs<'a> {
    mode: RequestMode,
    paths: &'a RequestPaths,
    existence: ArtifactExistence,
    projection_target: Option<&'static str>,
    fallback_preview: Option<&'a str>,
    execute_requested: bool,
    review_checkpoint: Option<ReviewEventCheckpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewState {
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
enum ReviewEventCheckpoint {
    PlanApproved,
    ExecuteRequested,
}

fn latest_review_event_checkpoint(event_texts: &[String]) -> Option<ReviewEventCheckpoint> {
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

fn classify_review_state(
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

fn review_state_messages(state: ReviewState) -> (&'static str, &'static str, bool) {
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

fn allowed_responses_for(mode: RequestMode, state: ReviewState) -> Vec<&'static str> {
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

fn artifact_path_for_kind<'a>(paths: &'a RequestPaths, kind: &str) -> &'a Path {
    match kind {
        "plan" => paths.plan.as_path(),
        "intent_alignment" => paths.intent_alignment.as_path(),
        _ => paths.request.as_path(),
    }
}

fn build_review_artifact_preview<F>(
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

fn derive_review_packet<F>(inputs: &ReviewPacketInputs<'_>, read_file: F) -> Value
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
    json!({
        "state": state.wire(),
        "artifact_kind": artifact_kind,
        "artifact_path": path_json(target_path),
        "artifact_exists": artifact_exists,
        "artifact_preview": preview,
        "prompt": prompt,
        "allowed_responses": allowed,
        "next_action": next_action,
        "execute_allowed": execute_allowed,
    })
}

fn parse_execute_requested(args: &Value) -> bool {
    args.get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || args
            .get("execute_after_approval")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

fn extract_mode_from_request_lisp(text: &str) -> RequestMode {
    if text.contains(":mode :trusted-agent") {
        RequestMode::TrustedAgent
    } else {
        RequestMode::HumanInteractive
    }
}

fn lisp_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests;
