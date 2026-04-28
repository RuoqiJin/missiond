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
                .with_suggestion("actions: start|advance|status"),
            ))
        }
    };

    match action {
        "start" => action_start(state, &args).await,
        "advance" => action_advance(state, &args).await,
        "status" => action_status(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_request action `{}`", other),
            )
            .with_suggestion("valid: start|advance|status"),
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
    let inputs = ReviewPacketInputs {
        mode,
        paths: &paths,
        existence,
        projection_target: None,
        fallback_preview: None,
        execute_requested: false,
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
            "review the returned intent/plan artifact, approve through mission_directive or mission_plan, then call mission_request(action=advance)"
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewState {
    Received,
    IntentDrafting,
    AwaitingIntentApproval,
    AwaitingPlanApproval,
    ExecuteRequested,
}

impl ReviewState {
    fn wire(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::IntentDrafting => "intent_drafting",
            Self::AwaitingIntentApproval => "awaiting_intent_approval",
            Self::AwaitingPlanApproval => "awaiting_plan_approval",
            Self::ExecuteRequested => "execute_requested",
        }
    }
}

fn classify_review_state(
    existence: ArtifactExistence,
    projection_target: Option<&'static str>,
    execute_requested: bool,
) -> (ReviewState, &'static str) {
    if existence.plan && execute_requested {
        (ReviewState::ExecuteRequested, "plan")
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
            "Plan execution requested; mission_plan execute path runs via the existing approved-plan flow.",
            "mission_plan execute path runs via the existing approved-plan flow",
            true,
        ),
        ReviewState::AwaitingPlanApproval => (
            "Review plan.lisp and approve via mission_plan, then call mission_request advance with execute=true.",
            "approve plan via mission_plan, then call mission_request advance with execute=true",
            false,
        ),
        ReviewState::AwaitingIntentApproval => (
            "Review intent-alignment.lisp and approve via mission_directive, then call mission_request advance.",
            "approve intent via mission_directive, then call mission_request advance",
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
        let (state, kind) = classify_review_state(existence, None, false);
        assert_eq!(state, ReviewState::AwaitingPlanApproval);
        assert_eq!(kind, "plan");
    }

    #[test]
    fn classify_review_state_plan_with_execute_yields_execute_requested() {
        let existence = ArtifactExistence {
            request: true,
            intent_alignment: false,
            plan: true,
        };
        let (state, kind) = classify_review_state(existence, None, true);
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
        let (state, kind) = classify_review_state(existence, None, false);
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
        let (state, kind) = classify_review_state(existence, Some("plan"), false);
        assert_eq!(state, ReviewState::IntentDrafting);
        assert_eq!(kind, "plan");
    }

    #[test]
    fn classify_review_state_default_is_received() {
        let existence = ArtifactExistence::default();
        let (state, kind) = classify_review_state(existence, None, false);
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
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "awaiting_intent_approval");
        assert_eq!(packet["artifact_kind"], "intent_alignment");
        assert_eq!(packet["artifact_exists"], true);
        assert_eq!(packet["execute_allowed"], false);
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
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "awaiting_plan_approval");
        assert_eq!(packet["artifact_kind"], "plan");
        assert_eq!(packet["execute_allowed"], false);
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
        };
        let packet = derive_review_packet(&inputs, no_read);
        assert_eq!(packet["state"], "execute_requested");
        assert_eq!(packet["execute_allowed"], true);
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
}
