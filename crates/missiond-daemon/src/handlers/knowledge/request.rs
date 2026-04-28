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
use chrono::{SecondsFormat, Utc};
use missiond_core::util::safe_byte_truncate;
use missiond_core::types::{CreateBoardTaskInput, PlanStatus};
use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::handlers::knowledge::file_artifacts::{
    atomic_write_artifact, resolve_writer_project_root, sanitize_topic_segment,
};
use crate::state::AppState;

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
    let request_paths = match (sanitized_request_id.as_deref(), project_root_result.as_ref()) {
        (Some(id), Ok(root)) => Some(request_paths_for(root, id)),
        _ => None,
    };

    let inner = super::unified_entry::run_pipeline(state, args.clone()).await?;

    let request_id_present = sanitized_request_id.is_some();
    let projection = run_projection(&inner, request_paths.as_ref(), overwrite, request_id_present);
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

fn resolve_directive_ref(args: &Value, intent_alignment_text: Option<&str>) -> Option<DirectiveRef> {
    let id = nonblank(args.get("approved_directive_id"))
        .or_else(|| nonblank(args.get("directive_id")));
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
    let id = extract_lisp_keyword_string(text, "directive_id")
        .or_else(|| extract_lisp_keyword_string(text, "id"))?;
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
    extract_lisp_keyword_string(text, "plan_id")
        .or_else(|| extract_lisp_keyword_string(text, "id"))
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

    for key in [
        "compiler_mode",
        "persist",
        "target_project",
        "dispatch_strategy",
        "parallelism",
        "write_file",
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
}

fn plan_materialization_to_json(m: &PlanMaterialization) -> Value {
    json!({
        "plan_id": m.plan_ref.id,
        "board_task_id": m.board_task_id,
        "version": m.version,
        "sexp_hash": m.sexp_hash,
        "board_task_created": m.board_task_created,
        "source": "request-local plan.lisp",
    })
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

    Ok(PlanMaterialization {
        plan_ref: PlanRef {
            id: plan_id.to_string(),
        },
        board_task_id: anchor.board_task_id,
        version,
        sexp_hash,
        board_task_created: anchor.board_task_created,
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
        .or_else(|| {
            args.get("execute_after_approval")
                .and_then(|v| v.as_bool())
        });

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

    if outcome != RespondOutcome::Blocked && decision.requires_directive_ref() && directive_ref.is_none() {
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
            Some(text) => match materialize_request_plan(state, args, &request_id, &paths, text).await {
                Ok(materialized) => {
                    plan_ref = Some(materialized.plan_ref.clone());
                    plan_materialization = Some(materialized);
                }
                Err(e) => {
                    outcome = RespondOutcome::Blocked;
                    blocked_reason = Some(format!("failed to materialize request-local plan.lisp: {:#}", e));
                }
            },
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
                    format!("failed to append review event {}: {:#}", event_path.display(), e),
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
        respond_result.insert("board_task_materialized".into(), json!(b.board_task_created));
        respond_result.insert(
            "board_task_materialization".into(),
            board_task_materialization_to_json(b),
        );
    }
    if let Some(m) = plan_materialization.as_ref() {
        respond_result.insert("plan_materialized".into(), json!(true));
        respond_result.insert("plan_materialization".into(), plan_materialization_to_json(m));
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
            obj.insert("plan_materialization".into(), plan_materialization_to_json(m));
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

fn tool_result_payload(result: &ToolResult) -> Value {
    match result.content.first() {
        Some(ToolContent::Text { text }) => {
            serde_json::from_str(text).unwrap_or_else(|_| json!({ "text": text }))
        }
        None => json!(null),
    }
}

/// Read the unified-entry decorator's sibling JSON (second content element)
/// to lift `pipeline_stage` + `artifact_refs.scope`. The decorator and the
/// planner-error path both append exactly this shape; if the inner result
/// does not carry it (e.g. unexpected ToolResult shape), the meta is empty
/// and projection routing falls back to `unknown_stage`.
fn extract_pipeline_meta(inner: &ToolResult) -> PipelineMeta {
    let meta_value = inner.content.get(1).and_then(|c| match c {
        ToolContent::Text { text } => serde_json::from_str::<Value>(text).ok(),
    });
    let pipeline_stage = meta_value
        .as_ref()
        .and_then(|v| v.get("pipeline_stage"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let artifact_scope = meta_value
        .as_ref()
        .and_then(|v| v.pointer("/artifact_refs/scope"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    PipelineMeta {
        pipeline_stage,
        artifact_scope,
    }
}

async fn resolve_request_project_root(
    state: &AppState,
    args: &Value,
) -> std::result::Result<PathBuf, String> {
    resolve_writer_project_root(
        &state.project_registry,
        nonblank(args.get("project")).as_deref(),
        nonblank(args.get("cwd")).as_deref(),
        nonblank(args.get("target_project")).as_deref(),
    )
    .await
}

#[derive(Debug, Clone)]
struct RequestPaths {
    request: PathBuf,
    intent_alignment: PathBuf,
    plan: PathBuf,
    events_dir: PathBuf,
    receipts_dir: PathBuf,
    reports_dir: PathBuf,
    initial_event: PathBuf,
}

fn request_paths_for(project_root: &Path, request_id: &str) -> RequestPaths {
    let base = project_root
        .join(".missiond")
        .join("requests")
        .join(sanitize_request_id(request_id));
    let events_dir = base.join("events");
    RequestPaths {
        request: base.join("request.lisp"),
        intent_alignment: base.join("intent-alignment.lisp"),
        plan: base.join("plan.lisp"),
        receipts_dir: base.join("receipts"),
        reports_dir: base.join("reports"),
        initial_event: events_dir.join("000001.event.lisp"),
        events_dir,
    }
}

fn build_artifact_paths_json(paths: &RequestPaths) -> Value {
    json!({
        "request": path_json(&paths.request),
        "intent_alignment": path_json(&paths.intent_alignment),
        "plan": path_json(&paths.plan),
        "events_dir": path_json(&paths.events_dir),
        "receipts_dir": path_json(&paths.receipts_dir),
        "reports_dir": path_json(&paths.reports_dir),
    })
}

fn build_artifact_existence(paths: &RequestPaths) -> Value {
    build_artifact_existence_with(paths, |p| p.exists())
}

fn build_artifact_existence_with<F: Fn(&Path) -> bool>(paths: &RequestPaths, exists: F) -> Value {
    json!({
        "request": exists(&paths.request),
        "intent_alignment": exists(&paths.intent_alignment),
        "plan": exists(&paths.plan),
        "events_dir": exists(&paths.events_dir),
        "receipts_dir": exists(&paths.receipts_dir),
        "reports_dir": exists(&paths.reports_dir),
    })
}

fn path_json(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

// ───────────────────────────────────────────────────────────────────────
// Request-local projection — mirrors stable inner compile payloads into
// .missiond/requests/<request_id>/{intent-alignment,plan}.lisp.
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PipelineMeta {
    pipeline_stage: Option<String>,
    artifact_scope: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionTarget {
    IntentAlignment,
    Plan,
    Execute,
    Unknown,
}

impl ProjectionTarget {
    fn artifact_kind(self) -> Option<&'static str> {
        match self {
            Self::IntentAlignment => Some("intent_alignment"),
            Self::Plan => Some("plan"),
            Self::Execute | Self::Unknown => None,
        }
    }
}

fn classify_projection_target(meta: &PipelineMeta) -> ProjectionTarget {
    if let Some(stage) = meta.pipeline_stage.as_deref() {
        match stage {
            "s1_message_intake" | "s3_alignment_review_gate" => {
                return ProjectionTarget::IntentAlignment
            }
            "s4_plan_authoring" | "s5_plan_review_gate" => return ProjectionTarget::Plan,
            "s6_execution_runner" => return ProjectionTarget::Execute,
            _ => {}
        }
    }
    match meta.artifact_scope.as_deref() {
        Some("directive") => ProjectionTarget::IntentAlignment,
        Some("plan") => ProjectionTarget::Plan,
        Some("execution") => ProjectionTarget::Execute,
        _ => ProjectionTarget::Unknown,
    }
}

fn extract_projected_sexp(inner_payload: &Value) -> Option<(String, &'static str)> {
    if let Some(s) = inner_payload.get("compiled_sexp").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            return Some((s.to_string(), "compiled_sexp"));
        }
    }
    if let Some(s) = inner_payload
        .get("compiled_sexp_preview")
        .and_then(|v| v.as_str())
    {
        if !s.is_empty() {
            return Some((s.to_string(), "compiled_sexp_preview"));
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionStatus {
    Written,
    SkippedExecuteStage,
    SkippedPipelineError,
    SkippedUnknownStage,
    SkippedNoSexp,
    SkippedNoRequestId,
    SkippedNoProjectRoot,
    WriteFailed,
}

impl ProjectionStatus {
    fn wire(self) -> &'static str {
        match self {
            Self::Written => "written",
            Self::SkippedExecuteStage => "skipped_execute_stage",
            Self::SkippedPipelineError => "skipped_pipeline_error",
            Self::SkippedUnknownStage => "skipped_unknown_stage",
            Self::SkippedNoSexp => "skipped_no_sexp",
            Self::SkippedNoRequestId => "skipped_no_request_id",
            Self::SkippedNoProjectRoot => "skipped_no_project_root",
            Self::WriteFailed => "write_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectionPlan {
    /// Pure decision: the projection cannot or should not write. `kind`
    /// echoes the projection target when one matched the pipeline stage but
    /// the inner payload lacked a sexp.
    Skip {
        status: ProjectionStatus,
        kind: Option<&'static str>,
    },
    /// The pipeline produced a stable sexp and a projection target. The
    /// caller still needs `RequestPaths` + overwrite to actually write it.
    Write {
        kind: &'static str,
        body: String,
        sexp_source: &'static str,
    },
}

/// Pure projection planner — no IO, no AppState. The outcome maps directly
/// onto the `projection.status` field surfaced in the wrapper response.
fn plan_projection(
    target: ProjectionTarget,
    inner_payload: &Value,
    inner_is_error: bool,
) -> ProjectionPlan {
    if inner_is_error {
        return ProjectionPlan::Skip {
            status: ProjectionStatus::SkippedPipelineError,
            kind: None,
        };
    }
    let kind = match target {
        ProjectionTarget::IntentAlignment => "intent_alignment",
        ProjectionTarget::Plan => "plan",
        ProjectionTarget::Execute => {
            return ProjectionPlan::Skip {
                status: ProjectionStatus::SkippedExecuteStage,
                kind: None,
            }
        }
        ProjectionTarget::Unknown => {
            return ProjectionPlan::Skip {
                status: ProjectionStatus::SkippedUnknownStage,
                kind: None,
            }
        }
    };
    match extract_projected_sexp(inner_payload) {
        Some((body, source)) => ProjectionPlan::Write {
            kind,
            body,
            sexp_source: source,
        },
        None => ProjectionPlan::Skip {
            status: ProjectionStatus::SkippedNoSexp,
            kind: Some(kind),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectionOutcome {
    status: ProjectionStatus,
    target: Option<&'static str>,
    sexp_source: Option<&'static str>,
    path: Option<PathBuf>,
    sha256: Option<String>,
    bytes: Option<u64>,
    created: Option<bool>,
    overwritten: Option<bool>,
    error: Option<String>,
}

impl ProjectionOutcome {
    fn skipped(status: ProjectionStatus, kind: Option<&'static str>) -> Self {
        Self {
            status,
            target: kind,
            sexp_source: None,
            path: None,
            sha256: None,
            bytes: None,
            created: None,
            overwritten: None,
            error: None,
        }
    }
}

fn projection_to_json(outcome: &ProjectionOutcome) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("status".into(), json!(outcome.status.wire()));
    if let Some(t) = outcome.target {
        obj.insert("target".into(), json!(t));
    }
    if let Some(src) = outcome.sexp_source {
        obj.insert("sexp_source".into(), json!(src));
    }
    if let Some(p) = outcome.path.as_ref() {
        obj.insert("path".into(), json!(path_json(p)));
    }
    if let Some(s) = outcome.sha256.as_ref() {
        obj.insert("sha256".into(), json!(s));
    }
    if let Some(b) = outcome.bytes {
        obj.insert("bytes".into(), json!(b));
    }
    if let Some(c) = outcome.created {
        obj.insert("created".into(), json!(c));
    }
    if let Some(o) = outcome.overwritten {
        obj.insert("overwritten".into(), json!(o));
    }
    if let Some(e) = outcome.error.as_ref() {
        obj.insert("error".into(), json!(e));
    }
    Value::Object(obj)
}

/// Glue between the pipeline result and request-local IO. Splits cleanly
/// into the pure planner (`plan_projection`) and the side-effecting writer
/// (`atomic_write_artifact`) so unit tests can pin the planner without
/// touching disk.
fn run_projection(
    inner: &ToolResult,
    request_paths: Option<&RequestPaths>,
    overwrite: bool,
    request_id_known: bool,
) -> ProjectionOutcome {
    let inner_is_error = inner.is_error.unwrap_or(false);
    let inner_payload = tool_result_payload(inner);
    let pipeline_meta = extract_pipeline_meta(inner);
    let target = classify_projection_target(&pipeline_meta);
    let plan = plan_projection(target, &inner_payload, inner_is_error);

    match plan {
        ProjectionPlan::Skip { status, kind } => ProjectionOutcome::skipped(status, kind),
        ProjectionPlan::Write {
            kind,
            body,
            sexp_source,
        } => {
            let paths = match request_paths {
                Some(p) => p,
                None => {
                    let status = if request_id_known {
                        ProjectionStatus::SkippedNoProjectRoot
                    } else {
                        ProjectionStatus::SkippedNoRequestId
                    };
                    return ProjectionOutcome::skipped(status, Some(kind));
                }
            };
            let target_path = match kind {
                "intent_alignment" => &paths.intent_alignment,
                "plan" => &paths.plan,
                _ => unreachable!("plan_projection guards kind to known targets"),
            };
            match atomic_write_artifact(target_path, &body, overwrite) {
                Ok(write) => ProjectionOutcome {
                    status: ProjectionStatus::Written,
                    target: Some(kind),
                    sexp_source: Some(sexp_source),
                    path: Some(write.path),
                    sha256: Some(write.sha256),
                    bytes: Some(write.bytes),
                    created: Some(write.created),
                    overwritten: Some(write.overwritten),
                    error: None,
                },
                Err(e) => ProjectionOutcome {
                    status: ProjectionStatus::WriteFailed,
                    target: Some(kind),
                    sexp_source: Some(sexp_source),
                    path: Some(target_path.clone()),
                    sha256: None,
                    bytes: None,
                    created: None,
                    overwritten: None,
                    error: Some(format!("{:#}", e)),
                },
            }
        }
    }
}

struct RequestDoc<'a> {
    request_id: &'a str,
    mode: RequestMode,
    source: &'a str,
    objective: &'a str,
    created_at: &'a str,
    paths: &'a RequestPaths,
}

fn build_request_lisp(doc: &RequestDoc<'_>) -> String {
    let mut out = String::new();
    let requires_intent = doc.mode == RequestMode::HumanInteractive;
    let requires_plan = doc.mode == RequestMode::HumanInteractive;
    let _ = writeln!(out, ";; MissionD request artifact.");
    let _ = writeln!(out, ";; Schema: {}", REQUEST_SCHEMA);
    let _ = writeln!(out, "(mission-request {}", doc.request_id);
    let _ = writeln!(out, "  :schema {}", lisp_string(REQUEST_SCHEMA));
    let _ = writeln!(out, "  :request_id {}", lisp_string(doc.request_id));
    let _ = writeln!(out, "  :source {}", lisp_string(doc.source));
    let _ = writeln!(out, "  :mode :{}", doc.mode.lisp());
    let _ = writeln!(out, "  :state :received");
    let _ = writeln!(out, "  :created_at {}", lisp_string(doc.created_at));
    let _ = writeln!(out, "  :objective {}", lisp_string(doc.objective));
    let _ = writeln!(out, "  :artifacts");
    let _ = writeln!(
        out,
        "    ((request :path {})",
        lisp_string(&doc.paths.request.to_string_lossy())
    );
    let _ = writeln!(
        out,
        "     (intent-alignment :path {})",
        lisp_string(&doc.paths.intent_alignment.to_string_lossy())
    );
    let _ = writeln!(
        out,
        "     (plan :path {})",
        lisp_string(&doc.paths.plan.to_string_lossy())
    );
    let _ = writeln!(
        out,
        "     (events :path {})",
        lisp_string(&doc.paths.events_dir.to_string_lossy())
    );
    let _ = writeln!(
        out,
        "     (receipts :path {})",
        lisp_string(&doc.paths.receipts_dir.to_string_lossy())
    );
    let _ = writeln!(
        out,
        "     (reports :path {}))",
        lisp_string(&doc.paths.reports_dir.to_string_lossy())
    );
    let _ = writeln!(out, "  :policy");
    let _ = writeln!(
        out,
        "    (:requires_intent_approval {}",
        if requires_intent { "true" } else { "false" }
    );
    let _ = writeln!(
        out,
        "     :requires_plan_approval {}",
        if requires_plan { "true" } else { "false" }
    );
    let _ = writeln!(
        out,
        "     :trusted_agent_fast_path {})",
        if doc.mode == RequestMode::TrustedAgent {
            "true"
        } else {
            "false"
        }
    );
    let _ = writeln!(out, "  :next_surface mission_directive");
    let _ = writeln!(
        out,
        "  :blueprint \".missiond/v3/missiond-blueprint.lisp\")"
    );
    out
}

fn build_event_lisp(request_id: &str, created_at: &str, kind: &str, objective: &str) -> String {
    format!(
        ";; MissionD lifecycle event.\n\
         ;; Schema: {schema}\n\
         (lifecycle-event {event_id}\n\
           :schema {schema_s}\n\
           :seq 1\n\
           :event_id {event_id}\n\
           :request_id {request_id_s}\n\
           :kind :{kind}\n\
           :actor (:role :orchestrator :id \"mission_request\")\n\
           :time {created_at_s}\n\
           :payload (:objective {objective_s})\n\
           :idempotency_key {idem})\n",
        schema = EVENT_SCHEMA,
        schema_s = lisp_string(EVENT_SCHEMA),
        event_id = lisp_string(&format!("evt-{}-000001", request_id)),
        request_id_s = lisp_string(request_id),
        kind = kind,
        created_at_s = lisp_string(created_at),
        objective_s = lisp_string(objective),
        idem = lisp_string(&format!("{}/{}", request_id, kind)),
    )
}

fn request_id_from_args(args: &Value) -> String {
    match nonblank(args.get("request_id")) {
        Some(id) => sanitize_request_id(&id),
        None => format!("req-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
    }
}

fn sanitize_request_id(raw: &str) -> String {
    let sanitized = sanitize_topic_segment(raw);
    if sanitized == "anonymous" {
        format!("req-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
    } else {
        sanitized
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestMode {
    HumanInteractive,
    TrustedAgent,
}

impl RequestMode {
    fn wire(self) -> &'static str {
        match self {
            Self::HumanInteractive => "human_interactive",
            Self::TrustedAgent => "trusted_agent",
        }
    }

    fn lisp(self) -> &'static str {
        match self {
            Self::HumanInteractive => "human-interactive",
            Self::TrustedAgent => "trusted-agent",
        }
    }
}

fn parse_mode(raw: Option<&str>) -> RequestMode {
    match raw.unwrap_or("human_interactive").trim() {
        "trusted_agent" | "trusted-agent" => RequestMode::TrustedAgent,
        _ => RequestMode::HumanInteractive,
    }
}

fn nonblank(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
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
    args.get("execute").and_then(|v| v.as_bool()).unwrap_or(false)
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
mod tests {
    use super::*;

    #[test]
    fn lisp_string_escapes_quotes_and_newlines() {
        assert_eq!(lisp_string("a\"b\nc"), "\"a\\\"b\\nc\"");
    }

    #[test]
    fn request_lisp_carries_v3_policy() {
        let paths = request_paths_for(Path::new("/tmp/project"), "req-abc");
        let body = build_request_lisp(&RequestDoc {
            request_id: "req-abc",
            mode: RequestMode::TrustedAgent,
            source: "user_request",
            objective: "Ship it",
            created_at: "2026-04-28T00:00:00Z",
            paths: &paths,
        });
        assert!(body.contains(":schema \"missiond.request.v1\""));
        assert!(body.contains(":mode :trusted-agent"));
        assert!(body.contains(":trusted_agent_fast_path true"));
        assert!(body.contains(".missiond/v3/missiond-blueprint.lisp"));
    }

    #[test]
    fn event_lisp_uses_v3_lifecycle_event_shape() {
        let body = build_event_lisp(
            "req-abc",
            "2026-04-28T00:00:00Z",
            "request_received",
            "Ship it",
        );
        assert!(body.contains("(lifecycle-event \"evt-req-abc-000001\""));
        assert!(body.contains(":schema \"missiond.lifecycle-event.v1\""));
        assert!(body.contains(":event_id \"evt-req-abc-000001\""));
        assert!(body.contains(":idempotency_key \"req-abc/request_received\""));
    }

    #[test]
    fn request_paths_use_v3_layout() {
        let paths = request_paths_for(Path::new("/repo"), "req-1");
        assert_eq!(
            paths.request,
            Path::new("/repo").join(".missiond/requests/req-1/request.lisp")
        );
        assert_eq!(
            paths.initial_event,
            Path::new("/repo").join(".missiond/requests/req-1/events/000001.event.lisp")
        );
        assert_eq!(
            paths.intent_alignment,
            Path::new("/repo").join(".missiond/requests/req-1/intent-alignment.lisp")
        );
        assert_eq!(
            paths.plan,
            Path::new("/repo").join(".missiond/requests/req-1/plan.lisp")
        );
    }

    // ── projection helpers — pure, no AppState / no IO ──────────────────

    #[test]
    fn classify_projection_uses_pipeline_stage_first() {
        let meta = PipelineMeta {
            pipeline_stage: Some("s1_message_intake".into()),
            artifact_scope: Some("plan".into()),
        };
        // pipeline_stage wins over scope when both are present.
        assert_eq!(
            classify_projection_target(&meta),
            ProjectionTarget::IntentAlignment
        );

        let meta = PipelineMeta {
            pipeline_stage: Some("s4_plan_authoring".into()),
            artifact_scope: None,
        };
        assert_eq!(classify_projection_target(&meta), ProjectionTarget::Plan);

        let meta = PipelineMeta {
            pipeline_stage: Some("s6_execution_runner".into()),
            artifact_scope: None,
        };
        assert_eq!(classify_projection_target(&meta), ProjectionTarget::Execute);
    }

    #[test]
    fn classify_projection_falls_back_to_scope() {
        let meta = PipelineMeta {
            pipeline_stage: None,
            artifact_scope: Some("directive".into()),
        };
        assert_eq!(
            classify_projection_target(&meta),
            ProjectionTarget::IntentAlignment
        );

        let meta = PipelineMeta {
            pipeline_stage: None,
            artifact_scope: Some("plan".into()),
        };
        assert_eq!(classify_projection_target(&meta), ProjectionTarget::Plan);

        let meta = PipelineMeta {
            pipeline_stage: None,
            artifact_scope: None,
        };
        assert_eq!(classify_projection_target(&meta), ProjectionTarget::Unknown);
    }

    #[test]
    fn extract_sexp_prefers_compiled_over_preview() {
        let payload = json!({
            "compiled_sexp": "(directive :ok)",
            "compiled_sexp_preview": "(directive-draft)",
        });
        let (body, source) = extract_projected_sexp(&payload).expect("sexp present");
        assert_eq!(body, "(directive :ok)");
        assert_eq!(source, "compiled_sexp");
    }

    #[test]
    fn extract_sexp_falls_back_to_preview() {
        let payload = json!({
            "compiled_sexp_preview": "(directive-draft)",
        });
        let (body, source) = extract_projected_sexp(&payload).expect("preview present");
        assert_eq!(body, "(directive-draft)");
        assert_eq!(source, "compiled_sexp_preview");
    }

    #[test]
    fn extract_sexp_returns_none_when_blank_or_missing() {
        assert!(extract_projected_sexp(&json!({})).is_none());
        assert!(extract_projected_sexp(&json!({ "compiled_sexp": "" })).is_none());
        assert!(extract_projected_sexp(&json!({
            "compiled_sexp": null,
            "compiled_sexp_preview": ""
        }))
        .is_none());
    }

    #[test]
    fn plan_projection_directive_preview_writes_intent_alignment() {
        let payload = json!({
            "status": "dry_run",
            "compiled_sexp_preview": "(directive-draft\n  :utterance \"do x\")\n",
        });
        let plan =
            plan_projection(ProjectionTarget::IntentAlignment, &payload, false);
        match plan {
            ProjectionPlan::Write {
                kind,
                body,
                sexp_source,
            } => {
                assert_eq!(kind, "intent_alignment");
                assert_eq!(sexp_source, "compiled_sexp_preview");
                assert!(body.contains("directive-draft"));
            }
            other => panic!("expected Write, got {:?}", other),
        }
    }

    #[test]
    fn plan_projection_plan_compile_writes_plan_body() {
        let payload = json!({
            "status": "compiled",
            "compiled_sexp": "(plan :board_task_id \"btk-1\")\n",
        });
        let plan = plan_projection(ProjectionTarget::Plan, &payload, false);
        match plan {
            ProjectionPlan::Write {
                kind,
                body,
                sexp_source,
            } => {
                assert_eq!(kind, "plan");
                assert_eq!(sexp_source, "compiled_sexp");
                assert!(body.contains("board_task_id"));
            }
            other => panic!("expected Write, got {:?}", other),
        }
    }

    #[test]
    fn plan_projection_skips_on_pipeline_error() {
        let payload = json!({});
        let plan =
            plan_projection(ProjectionTarget::IntentAlignment, &payload, true);
        assert_eq!(
            plan,
            ProjectionPlan::Skip {
                status: ProjectionStatus::SkippedPipelineError,
                kind: None,
            }
        );
    }

    #[test]
    fn plan_projection_skips_on_execute_target() {
        let payload = json!({ "compiled_sexp": "(execute)" });
        let plan = plan_projection(ProjectionTarget::Execute, &payload, false);
        assert_eq!(
            plan,
            ProjectionPlan::Skip {
                status: ProjectionStatus::SkippedExecuteStage,
                kind: None,
            }
        );
    }

    #[test]
    fn plan_projection_skips_on_unknown_target() {
        let payload = json!({ "compiled_sexp": "(?)" });
        let plan = plan_projection(ProjectionTarget::Unknown, &payload, false);
        assert_eq!(
            plan,
            ProjectionPlan::Skip {
                status: ProjectionStatus::SkippedUnknownStage,
                kind: None,
            }
        );
    }

    #[test]
    fn plan_projection_marks_no_sexp_when_payload_lacks_keys() {
        let payload = json!({ "status": "dry_run" });
        let plan = plan_projection(ProjectionTarget::Plan, &payload, false);
        assert_eq!(
            plan,
            ProjectionPlan::Skip {
                status: ProjectionStatus::SkippedNoSexp,
                kind: Some("plan"),
            }
        );
    }

    #[test]
    fn projection_outcome_to_json_omits_unset_fields() {
        let outcome = ProjectionOutcome::skipped(ProjectionStatus::SkippedNoSexp, Some("plan"));
        let v = projection_to_json(&outcome);
        assert_eq!(v["status"], "skipped_no_sexp");
        assert_eq!(v["target"], "plan");
        assert!(v.get("path").is_none());
        assert!(v.get("sha256").is_none());
        assert!(v.get("error").is_none());
    }

    #[test]
    fn build_artifact_paths_json_lists_all_six_keys() {
        let paths = request_paths_for(Path::new("/repo"), "req-x");
        let v = build_artifact_paths_json(&paths);
        for key in [
            "request",
            "intent_alignment",
            "plan",
            "events_dir",
            "receipts_dir",
            "reports_dir",
        ] {
            assert!(v.get(key).is_some(), "missing key {}", key);
            assert!(v[key].is_string(), "{} should be string", key);
        }
        assert!(v["intent_alignment"]
            .as_str()
            .unwrap()
            .ends_with("intent-alignment.lisp"));
        assert!(v["plan"].as_str().unwrap().ends_with("plan.lisp"));
    }

    #[test]
    fn build_artifact_existence_with_predicate_drives_booleans() {
        let paths = request_paths_for(Path::new("/repo"), "req-x");
        // Pin only request.lisp + events_dir as existing; everything else absent.
        let exists = |p: &Path| {
            p == paths.request.as_path() || p == paths.events_dir.as_path()
        };
        let v = build_artifact_existence_with(&paths, exists);
        assert_eq!(v["request"], true);
        assert_eq!(v["events_dir"], true);
        assert_eq!(v["intent_alignment"], false);
        assert_eq!(v["plan"], false);
        assert_eq!(v["receipts_dir"], false);
        assert_eq!(v["reports_dir"], false);
    }

    // ── review_packet helpers — pure derivation, no AppState / no IO ───
    fn paths_fixture() -> RequestPaths {
        request_paths_for(Path::new("/repo"), "req-rp")
    }

    fn no_read(_p: &Path) -> Option<String> {
        None
    }

    #[test]
    fn classify_review_state_plan_present_wins_over_intent() {
        let existence = ArtifactExistence {
            request: true,
            intent_alignment: true,
            plan: true,
        };
        let (state, kind) = classify_review_state(existence, None, false, None);
        assert_eq!(state, ReviewState::AwaitingPlanApproval);
        assert_eq!(kind, "plan");
    }

    #[test]
    fn classify_review_state_plan_approved_event_yields_awaiting_execution() {
        let existence = ArtifactExistence {
            request: true,
            intent_alignment: true,
            plan: true,
        };
        let (state, kind) = classify_review_state(
            existence,
            None,
            false,
            Some(ReviewEventCheckpoint::PlanApproved),
        );
        assert_eq!(state, ReviewState::AwaitingExecution);
        assert_eq!(kind, "plan");
    }

    #[test]
    fn classify_review_state_plan_with_execute_yields_execute_requested() {
        let existence = ArtifactExistence {
            request: true,
            intent_alignment: false,
            plan: true,
        };
        let (state, kind) = classify_review_state(existence, None, true, None);
        assert_eq!(state, ReviewState::ExecuteRequested);
        assert_eq!(kind, "plan");
    }

    #[test]
    fn classify_review_state_intent_only_yields_awaiting_intent() {
        let existence = ArtifactExistence {
            request: true,
            intent_alignment: true,
            plan: false,
        };
        let (state, kind) = classify_review_state(existence, None, false, None);
        assert_eq!(state, ReviewState::AwaitingIntentApproval);
        assert_eq!(kind, "intent_alignment");
    }

    #[test]
    fn classify_review_state_no_artifacts_with_projection_target_drafts() {
        let existence = ArtifactExistence {
            request: true,
            intent_alignment: false,
            plan: false,
        };
        let (state, kind) = classify_review_state(existence, Some("plan"), false, None);
        assert_eq!(state, ReviewState::IntentDrafting);
        assert_eq!(kind, "plan");
    }

    #[test]
    fn classify_review_state_default_is_received() {
        let existence = ArtifactExistence::default();
        let (state, kind) = classify_review_state(existence, None, false, None);
        assert_eq!(state, ReviewState::Received);
        assert_eq!(kind, "request");
    }

    #[test]
    fn allowed_responses_match_blueprint_for_human_interactive() {
        assert_eq!(
            allowed_responses_for(
                RequestMode::HumanInteractive,
                ReviewState::AwaitingIntentApproval
            ),
            vec!["approve_intent", "reject_intent", "ask_question"]
        );
        assert_eq!(
            allowed_responses_for(
                RequestMode::HumanInteractive,
                ReviewState::AwaitingPlanApproval
            ),
            vec!["approve_plan", "reject_plan", "ask_question"]
        );
        assert_eq!(
            allowed_responses_for(
                RequestMode::HumanInteractive,
                ReviewState::AwaitingExecution
            ),
            vec!["execute_plan", "ask_question"]
        );
        assert_eq!(
            allowed_responses_for(RequestMode::HumanInteractive, ReviewState::Received),
            vec!["observe"]
        );
    }

    #[test]
    fn allowed_responses_match_blueprint_for_trusted_agent() {
        assert_eq!(
            allowed_responses_for(
                RequestMode::TrustedAgent,
                ReviewState::AwaitingIntentApproval
            ),
            vec!["approve_intent", "ask_question"]
        );
        assert_eq!(
            allowed_responses_for(RequestMode::TrustedAgent, ReviewState::AwaitingPlanApproval),
            vec!["approve_plan", "ask_question"]
        );
        assert_eq!(
            allowed_responses_for(RequestMode::TrustedAgent, ReviewState::AwaitingExecution),
            vec!["execute_plan", "ask_question"]
        );
    }

    #[test]
    fn build_review_artifact_preview_truncates_on_utf8_boundary() {
        // 60 Chinese characters * 3 bytes each = 180 bytes; ask for 80 bytes.
        let cjk: String = std::iter::repeat('好').take(60).collect();
        let preview =
            build_review_artifact_preview(Path::new("/x"), false, Some(&cjk), no_read, 80)
                .expect("preview");
        // Each '好' = 3 bytes. 80 / 3 = 26 chars * 3 = 78 bytes.
        assert_eq!(preview.len(), 78);
        assert_eq!(preview.chars().count(), 26);
        // Round-trip must remain valid UTF-8 (already a String, but pin the
        // intent: every byte boundary is a char boundary).
        for (i, _) in preview.char_indices() {
            assert!(preview.is_char_boundary(i));
        }
    }

    #[test]
    fn build_review_artifact_preview_prefers_file_when_exists() {
        let read = |_p: &Path| Some("(plan :board_task_id \"btk-1\")\n".to_string());
        let preview =
            build_review_artifact_preview(Path::new("/x"), true, Some("(fallback)"), read, 480)
                .expect("preview");
        assert!(preview.contains("board_task_id"));
        assert!(!preview.contains("fallback"));
    }

    #[test]
    fn build_review_artifact_preview_falls_back_when_file_absent() {
        let preview = build_review_artifact_preview(
            Path::new("/x"),
            false,
            Some("(directive-draft)"),
            no_read,
            480,
        )
        .expect("preview");
        assert_eq!(preview, "(directive-draft)");
    }

    #[test]
    fn build_review_artifact_preview_returns_none_without_data() {
        let preview =
            build_review_artifact_preview(Path::new("/x"), false, None, no_read, 480);
        assert!(preview.is_none());
    }

    #[test]
    fn derive_review_packet_intent_only_state() {
        let paths = paths_fixture();
        let inputs = ReviewPacketInputs {
            mode: RequestMode::HumanInteractive,
            paths: &paths,
            existence: ArtifactExistence {
                request: true,
                intent_alignment: true,
                plan: false,
            },
            projection_target: Some("intent_alignment"),
            fallback_preview: Some("(directive-draft)"),
            execute_requested: false,
            review_checkpoint: None,
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "awaiting_intent_approval");
        assert_eq!(packet["artifact_kind"], "intent_alignment");
        assert_eq!(packet["artifact_exists"], true);
        assert_eq!(packet["execute_allowed"], false);
        assert_eq!(
            packet["next_action"],
            "call mission_request respond with response=approve_intent, reject_intent, or ask_question"
        );
        assert!(packet["artifact_path"]
            .as_str()
            .unwrap()
            .ends_with("intent-alignment.lisp"));
        let allowed: Vec<&str> = packet["allowed_responses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(allowed, vec!["approve_intent", "reject_intent", "ask_question"]);
    }

    #[test]
    fn derive_review_packet_plan_present_overrides_intent() {
        let paths = paths_fixture();
        let inputs = ReviewPacketInputs {
            mode: RequestMode::HumanInteractive,
            paths: &paths,
            existence: ArtifactExistence {
                request: true,
                intent_alignment: true,
                plan: true,
            },
            projection_target: Some("plan"),
            fallback_preview: Some("(plan :ok)"),
            execute_requested: false,
            review_checkpoint: None,
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "awaiting_plan_approval");
        assert_eq!(packet["artifact_kind"], "plan");
        assert_eq!(packet["execute_allowed"], false);
        assert_eq!(
            packet["next_action"],
            "call mission_request respond with response=approve_plan, reject_plan, or ask_question"
        );
    }

    #[test]
    fn derive_review_packet_plan_approved_allows_execute_plan() {
        let paths = paths_fixture();
        let inputs = ReviewPacketInputs {
            mode: RequestMode::HumanInteractive,
            paths: &paths,
            existence: ArtifactExistence {
                request: true,
                intent_alignment: true,
                plan: true,
            },
            projection_target: None,
            fallback_preview: Some("(plan :ok)"),
            execute_requested: false,
            review_checkpoint: Some(ReviewEventCheckpoint::PlanApproved),
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "awaiting_execution");
        assert_eq!(packet["execute_allowed"], true);
        assert_eq!(
            packet["next_action"],
            "call mission_request respond with response=execute_plan + execute=true"
        );
        let allowed: Vec<&str> = packet["allowed_responses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(allowed, vec!["execute_plan", "ask_question"]);
    }

    #[test]
    fn latest_review_event_checkpoint_uses_latest_relevant_event() {
        let events = vec![
            "(lifecycle-event :payload (:decision :approve_plan :outcome :dispatched))"
                .to_string(),
            "(lifecycle-event :payload (:decision :ask_question :outcome :recorded))"
                .to_string(),
        ];
        assert_eq!(latest_review_event_checkpoint(&events), None);

        let events = vec![
            "(lifecycle-event :payload (:decision :approve_plan :outcome :dispatched))"
                .to_string(),
        ];
        assert_eq!(
            latest_review_event_checkpoint(&events),
            Some(ReviewEventCheckpoint::PlanApproved)
        );

        let events = vec![
            "(lifecycle-event :payload (:decision :approve_plan :outcome :dispatched))"
                .to_string(),
            "(lifecycle-event :payload (:decision :execute_plan :outcome :blocked))"
                .to_string(),
        ];
        assert_eq!(
            latest_review_event_checkpoint(&events),
            Some(ReviewEventCheckpoint::PlanApproved)
        );
    }

    #[test]
    fn derive_review_packet_execute_requested_when_plan_and_execute() {
        let paths = paths_fixture();
        let inputs = ReviewPacketInputs {
            mode: RequestMode::HumanInteractive,
            paths: &paths,
            existence: ArtifactExistence {
                request: true,
                intent_alignment: false,
                plan: true,
            },
            projection_target: None,
            fallback_preview: None,
            execute_requested: true,
            review_checkpoint: None,
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "execute_requested");
        assert_eq!(packet["execute_allowed"], true);
        assert_eq!(
            packet["next_action"],
            "observe execution status through mission_request status and task receipts"
        );
        assert_eq!(packet["allowed_responses"][0], "observe");
    }

    #[test]
    fn derive_review_packet_received_default_when_no_artifacts() {
        let paths = paths_fixture();
        let inputs = ReviewPacketInputs {
            mode: RequestMode::HumanInteractive,
            paths: &paths,
            existence: ArtifactExistence {
                request: true,
                intent_alignment: false,
                plan: false,
            },
            projection_target: None,
            fallback_preview: None,
            execute_requested: false,
            review_checkpoint: None,
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "received");
        assert_eq!(packet["artifact_kind"], "request");
        assert_eq!(packet["execute_allowed"], false);
    }

    #[test]
    fn derive_review_packet_intent_drafting_when_projection_targets_but_no_file() {
        let paths = paths_fixture();
        let inputs = ReviewPacketInputs {
            mode: RequestMode::HumanInteractive,
            paths: &paths,
            existence: ArtifactExistence::default(),
            projection_target: Some("intent_alignment"),
            fallback_preview: Some("(directive-draft)"),
            execute_requested: false,
            review_checkpoint: None,
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "intent_drafting");
        assert_eq!(packet["artifact_kind"], "intent_alignment");
        assert_eq!(packet["artifact_exists"], false);
        assert_eq!(packet["artifact_preview"], "(directive-draft)");
    }

    #[test]
    fn derive_review_packet_uses_safe_byte_truncation_for_cjk_preview() {
        let paths = paths_fixture();
        // ~120 bytes of CJK should be safely truncated to ≤80 bytes via
        // safe_byte_truncate. We feed it as the fallback preview to keep the
        // test pure (no file IO). Use a small max via the helper directly is
        // already covered above; here we just confirm derive_review_packet
        // does not panic on multi-byte input and produces a UTF-8 string.
        let cjk: String = std::iter::repeat('字').take(200).collect();
        let inputs = ReviewPacketInputs {
            mode: RequestMode::HumanInteractive,
            paths: &paths,
            existence: ArtifactExistence::default(),
            projection_target: Some("intent_alignment"),
            fallback_preview: Some(&cjk),
            execute_requested: false,
            review_checkpoint: None,
        };
        let packet = derive_review_packet(&inputs, no_read);
        let preview = packet["artifact_preview"].as_str().expect("preview present");
        assert!(preview.len() <= REVIEW_PREVIEW_MAX_BYTES);
        for (i, _) in preview.char_indices() {
            assert!(preview.is_char_boundary(i));
        }
    }

    #[test]
    fn derive_review_packet_reads_artifact_file_via_callback() {
        let paths = paths_fixture();
        let inputs = ReviewPacketInputs {
            mode: RequestMode::HumanInteractive,
            paths: &paths,
            existence: ArtifactExistence {
                request: true,
                intent_alignment: false,
                plan: true,
            },
            projection_target: None,
            fallback_preview: None,
            execute_requested: false,
            review_checkpoint: None,
        };
        let read = |_p: &Path| Some("(plan :from-disk true)".to_string());
        let packet = derive_review_packet(&inputs, read);
        assert_eq!(packet["state"], "awaiting_plan_approval");
        let preview = packet["artifact_preview"].as_str().expect("preview");
        assert!(preview.contains("from-disk"));
    }

    #[test]
    fn parse_execute_requested_handles_aliases() {
        assert!(!parse_execute_requested(&json!({})));
        assert!(parse_execute_requested(&json!({ "execute": true })));
        assert!(parse_execute_requested(
            &json!({ "execute_after_approval": true })
        ));
        assert!(!parse_execute_requested(&json!({ "execute": false })));
    }

    #[test]
    fn extract_mode_from_request_lisp_recognizes_trusted_agent() {
        let trusted = "(mission-request foo\n  :mode :trusted-agent\n  :state :received)";
        assert_eq!(
            extract_mode_from_request_lisp(trusted),
            RequestMode::TrustedAgent
        );
        let human = "(mission-request foo\n  :mode :human-interactive)";
        assert_eq!(
            extract_mode_from_request_lisp(human),
            RequestMode::HumanInteractive
        );
        // Default safe-side: anything that isn't trusted-agent stays human-interactive.
        assert_eq!(
            extract_mode_from_request_lisp(""),
            RequestMode::HumanInteractive
        );
    }

    #[test]
    fn extract_pipeline_meta_reads_decorator_sibling() {
        let inner_payload = json!({
            "status": "dry_run",
            "compiled_sexp_preview": "(directive-draft)",
        });
        let meta = json!({
            "pipeline_stage": "s1_message_intake",
            "artifact_refs": { "scope": "directive" },
        });
        let result = ToolResult {
            content: vec![
                ToolContent::Text {
                    text: serde_json::to_string(&inner_payload).unwrap(),
                },
                ToolContent::Text {
                    text: serde_json::to_string(&meta).unwrap(),
                },
            ],
            is_error: None,
        };
        let extracted = extract_pipeline_meta(&result);
        assert_eq!(
            extracted.pipeline_stage.as_deref(),
            Some("s1_message_intake")
        );
        assert_eq!(extracted.artifact_scope.as_deref(), Some("directive"));
    }

    // ── respond decision parsing — pure, no AppState ──────────────────

    #[test]
    fn parse_respond_decision_accepts_response_field() {
        let cases = [
            ("approve_intent", RespondDecision::ApproveIntent),
            ("reject_intent", RespondDecision::RejectIntent),
            ("ask_question", RespondDecision::AskQuestion),
            ("approve_plan", RespondDecision::ApprovePlan),
            ("reject_plan", RespondDecision::RejectPlan),
            ("execute_plan", RespondDecision::ExecutePlan),
        ];
        for (wire, expected) in cases {
            let parsed = parse_respond_decision(&json!({ "response": wire }))
                .expect("decision should parse");
            assert_eq!(parsed, expected, "wire `{}`", wire);
            assert_eq!(parsed.wire(), wire);
        }
    }

    #[test]
    fn parse_respond_decision_accepts_decision_alias() {
        let parsed = parse_respond_decision(&json!({ "decision": "approve_plan" }))
            .expect("decision should parse via alias");
        assert_eq!(parsed, RespondDecision::ApprovePlan);
    }

    #[test]
    fn parse_respond_decision_response_wins_over_alias() {
        let parsed = parse_respond_decision(&json!({
            "response": "execute_plan",
            "decision": "approve_intent",
        }))
        .expect("decision should parse");
        assert_eq!(parsed, RespondDecision::ExecutePlan);
    }

    #[test]
    fn parse_respond_decision_missing_returns_missing_param() {
        let err = parse_respond_decision(&json!({})).unwrap_err();
        assert_eq!(err, RespondParseError::Missing);
        let tool_err = err.into_tool_error();
        assert_eq!(tool_err.error_code, error_codes::MISSING_PARAM);
    }

    #[test]
    fn parse_respond_decision_unknown_returns_invalid_param() {
        let err = parse_respond_decision(&json!({ "response": "approve_workflow" })).unwrap_err();
        assert!(matches!(err, RespondParseError::Unknown(_)));
        let tool_err = err.into_tool_error();
        assert_eq!(tool_err.error_code, error_codes::INVALID_PARAM);
    }

    #[test]
    fn respond_decision_classification_matches_routing_table() {
        // approve_intent / reject_intent need a directive ref.
        for d in [RespondDecision::ApproveIntent, RespondDecision::RejectIntent] {
            assert!(d.requires_directive_ref());
            assert!(!d.requires_plan_ref());
        }
        // approve_plan / reject_plan / execute_plan need a plan ref.
        for d in [
            RespondDecision::ApprovePlan,
            RespondDecision::RejectPlan,
            RespondDecision::ExecutePlan,
        ] {
            assert!(!d.requires_directive_ref());
            assert!(d.requires_plan_ref());
        }
        // record-only routes — no directive/plan mutation, only event ledger.
        for d in [
            RespondDecision::RejectIntent,
            RespondDecision::RejectPlan,
            RespondDecision::AskQuestion,
        ] {
            assert!(d.record_only());
        }
        // approve_intent / approve_plan / execute_plan dispatch through the
        // existing inner surfaces.
        for d in [
            RespondDecision::ApproveIntent,
            RespondDecision::ApprovePlan,
            RespondDecision::ExecutePlan,
        ] {
            assert!(!d.record_only());
        }
    }

    // ── ref resolution — pure, no IO ──────────────────────────────────

    #[test]
    fn extract_lisp_keyword_string_finds_quoted_value() {
        let text = "(directive\n  :directive_id \"abc-123\"\n  :version 4)";
        assert_eq!(
            extract_lisp_keyword_string(text, "directive_id"),
            Some("abc-123".to_string())
        );
    }

    #[test]
    fn extract_lisp_keyword_int_finds_numeric_value() {
        let text = "(directive\n  :directive_id \"abc-123\"\n  :version 4)";
        assert_eq!(extract_lisp_keyword_int(text, "version"), Some(4));
    }

    #[test]
    fn extract_lisp_keyword_string_returns_none_when_missing() {
        let text = "(directive :goal :ship)";
        assert!(extract_lisp_keyword_string(text, "directive_id").is_none());
        assert!(extract_lisp_keyword_int(text, "version").is_none());
    }

    #[test]
    fn extract_directive_ref_from_artifact_round_trip() {
        let text = "(directive :directive_id \"00000000-0000-0000-0000-000000000abc\" :directive_version 7)";
        let parsed = extract_directive_ref_from_artifact(text).expect("ref present");
        assert_eq!(parsed.id, "00000000-0000-0000-0000-000000000abc");
        assert_eq!(parsed.version, 7);
    }

    #[test]
    fn resolve_directive_ref_prefers_explicit_args_over_artifact() {
        let args = json!({
            "approved_directive_id": "explicit-uuid",
            "directive_version": 9,
        });
        let artifact = "(directive :directive_id \"artifact-uuid\" :version 1)";
        let resolved = resolve_directive_ref(&args, Some(artifact)).expect("ref resolves");
        assert_eq!(resolved.id, "explicit-uuid");
        assert_eq!(resolved.version, 9);
    }

    #[test]
    fn resolve_directive_ref_falls_back_to_artifact_when_args_missing() {
        let args = json!({});
        let artifact = "(directive :directive_id \"artifact-uuid\" :version 3)";
        let resolved = resolve_directive_ref(&args, Some(artifact)).expect("ref resolves");
        assert_eq!(resolved.id, "artifact-uuid");
        assert_eq!(resolved.version, 3);
    }

    #[test]
    fn resolve_directive_ref_returns_none_without_id_or_version() {
        let args = json!({});
        // Artifact lacks :directive_id / :version → blocked.
        let artifact = "(directive :goal :ship)";
        assert!(resolve_directive_ref(&args, Some(artifact)).is_none());
        // Args carry id but no version → still blocked (mission_directive
        // approve requires both).
        let args = json!({ "approved_directive_id": "x" });
        assert!(resolve_directive_ref(&args, None).is_none());
    }

    #[test]
    fn resolve_plan_ref_prefers_args_then_artifact_then_blocks() {
        // Explicit arg wins.
        let args = json!({ "approved_plan_id": "explicit-plan" });
        let resolved =
            resolve_plan_ref(&args, Some("(plan :plan_id \"artifact-plan\")"), &[])
                .expect("plan ref");
        assert_eq!(resolved.id, "explicit-plan");
        // Falls back to artifact when args missing.
        let resolved =
            resolve_plan_ref(&json!({}), Some("(plan :plan_id \"artifact-plan\")"), &[])
                .expect("plan ref");
        assert_eq!(resolved.id, "artifact-plan");
        // Blocked when both missing.
        assert!(resolve_plan_ref(&json!({}), Some("(plan :goal :ship)"), &[]).is_none());
        assert!(resolve_plan_ref(&json!({}), None, &[]).is_none());
    }

    #[test]
    fn resolve_plan_ref_falls_back_to_latest_review_event() {
        let events = vec![
            "(event :plan_id \"old-plan\")".to_string(),
            "(event :decision :approve_plan :plan_id \"new-plan\")".to_string(),
        ];
        let resolved = resolve_plan_ref(&json!({}), Some("(plan :goal :ship)"), &events)
            .expect("event ref");
        assert_eq!(resolved.id, "new-plan");
    }

    #[test]
    fn plan_materialization_json_exposes_ref_and_anchor() {
        let m = PlanMaterialization {
            plan_ref: PlanRef { id: "p1".into() },
            board_task_id: "b1".into(),
            version: 2,
            sexp_hash: "abc".into(),
            board_task_created: true,
        };
        let v = plan_materialization_to_json(&m);
        assert_eq!(v["plan_id"], "p1");
        assert_eq!(v["board_task_id"], "b1");
        assert_eq!(v["version"], 2);
        assert_eq!(v["board_task_created"], true);
    }

    #[test]
    fn respond_plan_compile_args_default_board_task_to_request_id() {
        let directive = DirectiveRef {
            id: "00000000-0000-0000-0000-000000000abc".into(),
            version: 7,
        };
        let args = json!({
            "compiler_mode": "dry_run",
            "project": "missiond",
            "persist": false,
            "directive_version": 99,
        });
        let out = build_respond_plan_compile_args(&args, &directive, "req-123");

        assert_eq!(out["approved_directive_id"], directive.id);
        assert_eq!(out["directive_version"], 7);
        assert_eq!(out["board_task_id"], "req-123");
        assert_eq!(out["compiler_mode"], "dry_run");
        assert_eq!(out["project"], "missiond");
        assert_eq!(out["persist"], false);
    }

    #[test]
    fn respond_plan_compile_args_use_explicit_board_task() {
        let directive = DirectiveRef {
            id: "00000000-0000-0000-0000-000000000abc".into(),
            version: 1,
        };
        let args = json!({
            "board_task_id": "btk-42",
            "target_project": "missiond",
            "write_file": true,
            "overwrite_file": true,
            "review_gate_policy": "manual",
        });
        let out = build_respond_plan_compile_args(&args, &directive, "req-123");

        assert_eq!(out["board_task_id"], "btk-42");
        assert_eq!(out["target_project"], "missiond");
        assert_eq!(out["write_file"], true);
        assert_eq!(out["overwrite_file"], true);
        assert_eq!(out["review_gate_policy"], "manual");
    }

    // ── event sequencing — pure ────────────────────────────────────────

    #[test]
    fn next_event_seq_starts_after_initial_request_received_event() {
        // Only the initial request_received event has landed.
        let names = vec!["000001.event.lisp".to_string()];
        assert_eq!(next_event_seq(&names), 2);
    }

    #[test]
    fn next_event_seq_picks_max_plus_one() {
        let names = vec![
            "000001.event.lisp".to_string(),
            "000002.event.lisp".to_string(),
            "000007.event.lisp".to_string(),
            "stray.txt".to_string(),
        ];
        assert_eq!(next_event_seq(&names), 8);
    }

    #[test]
    fn next_event_seq_ignores_unrelated_filenames() {
        let names = vec![
            "README.md".to_string(),
            "000003.event.lisp.bak".to_string(),
            "abc.event.lisp".to_string(),
        ];
        // None match the strict <digits>.event.lisp pattern.
        assert_eq!(next_event_seq(&names), 1);
    }

    #[test]
    fn next_event_seq_with_no_existing_events_starts_at_one() {
        let names: Vec<String> = Vec::new();
        assert_eq!(next_event_seq(&names), 1);
    }

    #[test]
    fn event_path_for_seq_zero_pads_to_six_digits() {
        let path = event_path_for_seq(Path::new("/repo/.missiond/requests/req-x/events"), 5);
        assert_eq!(
            path,
            Path::new("/repo/.missiond/requests/req-x/events/000005.event.lisp")
        );
    }

    // ── review event lisp — pure render ───────────────────────────────

    #[test]
    fn build_review_event_lisp_records_dispatched_approve_intent() {
        let directive = DirectiveRef {
            id: "00000000-0000-0000-0000-000000000abc".into(),
            version: 4,
        };
        let body = build_review_event_lisp(&ReviewEventArgs {
            request_id: "req-rp",
            seq: 2,
            decision: RespondDecision::ApproveIntent,
            outcome: RespondOutcome::Dispatched,
            note: Some("looks good"),
            directive_ref: Some(&directive),
            plan_ref: None,
            execute: false,
            inner_action: Some("mission_directive::approve"),
            blocked_reason: None,
            created_at: "2026-04-28T00:00:00Z",
        });
        assert!(body.contains(":kind :review_response_dispatched"));
        assert!(body.contains(":decision :approve_intent"));
        assert!(body.contains(":outcome :dispatched"));
        assert!(body.contains(":directive_id \"00000000-0000-0000-0000-000000000abc\""));
        assert!(body.contains(":directive_version 4"));
        assert!(body.contains(":note \"looks good\""));
        assert!(body.contains(":execute false"));
        assert!(body.contains(":inner_action \"mission_directive::approve\""));
        assert!(body.contains(":idempotency_key \"req-rp/review_response_dispatched/000002\""));
    }

    #[test]
    fn build_review_event_lisp_records_blocked_missing_plan_ref() {
        let body = build_review_event_lisp(&ReviewEventArgs {
            request_id: "req-rp",
            seq: 3,
            decision: RespondDecision::ExecutePlan,
            outcome: RespondOutcome::Blocked,
            note: None,
            directive_ref: None,
            plan_ref: None,
            execute: false,
            inner_action: None,
            blocked_reason: Some("plan ref missing"),
            created_at: "2026-04-28T00:00:00Z",
        });
        assert!(body.contains(":kind :review_response_blocked"));
        assert!(body.contains(":decision :execute_plan"));
        assert!(body.contains(":outcome :blocked"));
        assert!(body.contains(":blocked_reason \"plan ref missing\""));
        // Refs absent — ensure we did not invent fields.
        assert!(!body.contains(":directive_id"));
        assert!(!body.contains(":plan_id"));
    }

    #[test]
    fn build_review_event_lisp_record_only_reject_plan_no_inner_action() {
        let plan = PlanRef {
            id: "11111111-1111-1111-1111-111111111111".into(),
        };
        let body = build_review_event_lisp(&ReviewEventArgs {
            request_id: "req-rp",
            seq: 4,
            decision: RespondDecision::RejectPlan,
            outcome: RespondOutcome::Recorded,
            note: Some("wrong scope"),
            directive_ref: None,
            plan_ref: Some(&plan),
            execute: false,
            inner_action: None,
            blocked_reason: None,
            created_at: "2026-04-28T00:00:00Z",
        });
        assert!(body.contains(":kind :review_response_recorded"));
        assert!(body.contains(":decision :reject_plan"));
        assert!(body.contains(":outcome :recorded"));
        assert!(body.contains(":plan_id \"11111111-1111-1111-1111-111111111111\""));
        assert!(body.contains(":note \"wrong scope\""));
        // reject_plan must NOT mutate approval state — no inner_action stamp.
        assert!(!body.contains(":inner_action"));
    }

    // ── next_action vocabulary — pure ─────────────────────────────────

    #[test]
    fn next_action_dispatched_paths_describe_continuation() {
        assert!(next_action_for(
            RespondDecision::ApproveIntent,
            RespondOutcome::Dispatched,
        )
        .contains("plan.lisp"));
        assert!(next_action_for(
            RespondDecision::ApprovePlan,
            RespondOutcome::Dispatched,
        )
        .contains("execute_plan"));
        assert!(next_action_for(
            RespondDecision::ExecutePlan,
            RespondOutcome::Dispatched,
        )
        .contains("execute"));
    }

    #[test]
    fn next_action_blocked_message_describes_remediation() {
        let msg = next_action_for(RespondDecision::ApproveIntent, RespondOutcome::Blocked);
        assert!(msg.contains("missing"));
    }

    #[test]
    fn next_action_record_only_paths_describe_followup() {
        assert!(next_action_for(
            RespondDecision::RejectIntent,
            RespondOutcome::Recorded,
        )
        .contains("revise"));
        assert!(next_action_for(
            RespondDecision::AskQuestion,
            RespondOutcome::Recorded,
        )
        .contains("question"));
    }

    // ── parse_event_seq_from_filename strictness ──────────────────────

    #[test]
    fn parse_event_seq_only_accepts_numeric_stem() {
        assert_eq!(parse_event_seq_from_filename("000001.event.lisp"), Some(1));
        assert_eq!(parse_event_seq_from_filename("999999.event.lisp"), Some(999999));
        assert!(parse_event_seq_from_filename("abc.event.lisp").is_none());
        assert!(parse_event_seq_from_filename("000001.event.lisp.bak").is_none());
        assert!(parse_event_seq_from_filename("000001.lisp").is_none());
        assert!(parse_event_seq_from_filename("").is_none());
    }
}
