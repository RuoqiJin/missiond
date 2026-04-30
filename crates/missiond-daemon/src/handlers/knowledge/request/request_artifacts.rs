use chrono::{SecondsFormat, Utc};
use missiond_mcp::tools::{ToolContent, ToolResult};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::handlers::knowledge::file_artifacts::{
    atomic_write_artifact, resolve_writer_project_root, sanitize_topic_segment,
};
use crate::state::AppState;

use super::{lisp_string, EVENT_SCHEMA, REQUEST_SCHEMA};

pub(super) fn tool_result_payload(result: &ToolResult) -> Value {
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
pub(super) fn extract_pipeline_meta(inner: &ToolResult) -> PipelineMeta {
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

pub(super) async fn resolve_request_project_root(
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
pub(super) struct RequestPaths {
    pub(super) request: PathBuf,
    pub(super) intent_alignment: PathBuf,
    pub(super) plan: PathBuf,
    pub(super) events_dir: PathBuf,
    pub(super) receipts_dir: PathBuf,
    pub(super) reports_dir: PathBuf,
    pub(super) initial_event: PathBuf,
}

pub(super) fn request_paths_for(project_root: &Path, request_id: &str) -> RequestPaths {
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

pub(super) fn build_artifact_paths_json(paths: &RequestPaths) -> Value {
    json!({
        "request": path_json(&paths.request),
        "intent_alignment": path_json(&paths.intent_alignment),
        "plan": path_json(&paths.plan),
        "events_dir": path_json(&paths.events_dir),
        "receipts_dir": path_json(&paths.receipts_dir),
        "reports_dir": path_json(&paths.reports_dir),
    })
}

pub(super) fn build_artifact_existence(paths: &RequestPaths) -> Value {
    build_artifact_existence_with(paths, |p| p.exists())
}

pub(super) fn build_artifact_existence_with<F: Fn(&Path) -> bool>(
    paths: &RequestPaths,
    exists: F,
) -> Value {
    json!({
        "request": exists(&paths.request),
        "intent_alignment": exists(&paths.intent_alignment),
        "plan": exists(&paths.plan),
        "events_dir": exists(&paths.events_dir),
        "receipts_dir": exists(&paths.receipts_dir),
        "reports_dir": exists(&paths.reports_dir),
    })
}

pub(super) fn path_json(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

// ───────────────────────────────────────────────────────────────────────
// Request-local projection — mirrors stable inner compile payloads into
// .missiond/requests/<request_id>/{intent-alignment,plan}.lisp.
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PipelineMeta {
    pub(super) pipeline_stage: Option<String>,
    pub(super) artifact_scope: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProjectionTarget {
    IntentAlignment,
    Plan,
    Execute,
    Unknown,
}

pub(super) fn classify_projection_target(meta: &PipelineMeta) -> ProjectionTarget {
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

pub(super) fn extract_projected_sexp(inner_payload: &Value) -> Option<(String, &'static str)> {
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

pub(super) fn enrich_intent_alignment_projection(body: String, inner_payload: &Value) -> String {
    let directive_id = match nonblank(inner_payload.get("directive_id")) {
        Some(id) => id,
        None => return body,
    };
    let version = match inner_payload.get("version").and_then(|v| v.as_i64()) {
        Some(v) => v,
        None => return body,
    };
    if body.contains(":directive_id")
        && (body.contains(":version") || body.contains(":directive_version"))
    {
        return body;
    }

    let trimmed_len = body.trim_end().len();
    let trailing = body[trimmed_len..].to_string();
    let mut core = body[..trimmed_len].to_string();
    if !core.ends_with(')') {
        return body;
    }
    core.pop();
    if !core.contains(":directive_id") {
        let _ = write!(core, "\n  :directive_id {}", lisp_string(&directive_id));
    }
    if !core.contains(":version") && !core.contains(":directive_version") {
        let _ = write!(core, "\n  :version {}", version);
    }
    core.push(')');
    core.push_str(&trailing);
    core
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProjectionStatus {
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
    pub(super) fn wire(self) -> &'static str {
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
pub(super) enum ProjectionPlan {
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
pub(super) fn plan_projection(
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
        Some((body, source)) => {
            let body = if target == ProjectionTarget::IntentAlignment {
                enrich_intent_alignment_projection(body, inner_payload)
            } else {
                body
            };
            ProjectionPlan::Write {
                kind,
                body,
                sexp_source: source,
            }
        }
        None => ProjectionPlan::Skip {
            status: ProjectionStatus::SkippedNoSexp,
            kind: Some(kind),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectionOutcome {
    pub(super) status: ProjectionStatus,
    pub(super) target: Option<&'static str>,
    pub(super) sexp_source: Option<&'static str>,
    pub(super) path: Option<PathBuf>,
    sha256: Option<String>,
    pub(super) bytes: Option<u64>,
    pub(super) created: Option<bool>,
    pub(super) overwritten: Option<bool>,
    pub(super) error: Option<String>,
}

impl ProjectionOutcome {
    pub(super) fn skipped(status: ProjectionStatus, kind: Option<&'static str>) -> Self {
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

pub(super) fn projection_to_json(outcome: &ProjectionOutcome) -> Value {
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
pub(super) fn run_projection(
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

pub(super) struct RequestDoc<'a> {
    pub(super) request_id: &'a str,
    pub(super) mode: RequestMode,
    pub(super) source: &'a str,
    pub(super) objective: &'a str,
    pub(super) created_at: &'a str,
    pub(super) paths: &'a RequestPaths,
}

pub(super) fn build_request_lisp(doc: &RequestDoc<'_>) -> String {
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

pub(super) fn build_event_lisp(
    request_id: &str,
    created_at: &str,
    kind: &str,
    objective: &str,
) -> String {
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

pub(super) fn request_id_from_args(args: &Value) -> String {
    match nonblank(args.get("request_id")) {
        Some(id) => sanitize_request_id(&id),
        None => format!("req-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
    }
}

pub(super) fn sanitize_request_id(raw: &str) -> String {
    let sanitized = sanitize_topic_segment(raw);
    if sanitized == "anonymous" {
        format!("req-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
    } else {
        sanitized
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestMode {
    HumanInteractive,
    TrustedAgent,
}

impl RequestMode {
    pub(super) fn wire(self) -> &'static str {
        match self {
            Self::HumanInteractive => "human_interactive",
            Self::TrustedAgent => "trusted_agent",
        }
    }

    pub(super) fn lisp(self) -> &'static str {
        match self {
            Self::HumanInteractive => "human-interactive",
            Self::TrustedAgent => "trusted-agent",
        }
    }
}

pub(super) fn parse_mode(raw: Option<&str>) -> RequestMode {
    match raw.unwrap_or("human_interactive").trim() {
        "trusted_agent" | "trusted-agent" => RequestMode::TrustedAgent,
        _ => RequestMode::HumanInteractive,
    }
}

pub(super) fn nonblank(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub(super) fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
