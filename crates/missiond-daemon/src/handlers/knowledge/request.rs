//! mission_request — v3 unified request entry.
//!
//! Lisp authority:
//!   - .missiond/v3/missiond-blueprint.lisp :: mission_request surface
//!   - .missiond/v3/missiond-blueprint.lisp :: unified-entry state-machine
//!   - .missiond/v3/missiond-blueprint.lisp :: artifact mission-request /
//!     lifecycle-event
//!
//! v0 is intentionally conservative:
//!   - file-first request.lisp + initial lifecycle event;
//!   - no DB schema migration;
//!   - no auto-approval of intent or plan;
//!   - no direct workstation dispatch;
//!   - all actual directive/plan work is delegated to the existing
//!     unified_entry helper, which itself composes mission_directive and
//!     mission_plan.

use anyhow::Result;
use chrono::{SecondsFormat, Utc};
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

    if write_request_file {
        let root = match resolve_request_project_root(state, args).await {
            Ok(root) => root,
            Err(reason) => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(error_codes::INVALID_PARAM, reason).with_suggestion(
                        "pass project, absolute cwd, or target_project; mission_request refuses process-cwd fallback",
                    ),
                ))
            }
        };

        let paths = request_paths(&root, &request_id);
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
            "artifact_paths": {
                "request": path_json(&paths.request),
                "intent_alignment": path_json(&paths.intent_alignment),
                "plan": path_json(&paths.plan),
                "events_dir": path_json(&paths.events_dir),
                "receipts_dir": path_json(&paths.receipts_dir),
                "reports_dir": path_json(&paths.reports_dir),
            }
        });
    }

    let mut pipeline_args = args.clone();
    normalize_start_args(&mut pipeline_args, &request_id);
    let inner = super::unified_entry::run_pipeline(state, pipeline_args).await?;
    Ok(wrap_pipeline_result("start", mode, file_payload, inner))
}

async fn action_advance(state: &AppState, args: &Value) -> Result<ToolResult> {
    let request_id = nonblank(args.get("request_id"));
    let mode = parse_mode(args.get("mode").and_then(|v| v.as_str()));
    let inner = super::unified_entry::run_pipeline(state, args.clone()).await?;
    let file_payload = json!({
        "request_id": request_id,
        "request_written": false,
        "event_written": false,
    });
    Ok(wrap_pipeline_result("advance", mode, file_payload, inner))
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
    let paths = request_paths(&root, &request_id);
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

    Ok(ToolResult::json_pretty(&json!({
        "status": "ok",
        "action": "status",
        "request_id": request_id,
        "request_path": path_json(&paths.request),
        "request_lisp": text,
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
    inner: ToolResult,
) -> ToolResult {
    let inner_is_error = inner.is_error.unwrap_or(false);
    let inner_payload = tool_result_payload(&inner);
    let mut response = json!({
        "status": if inner_is_error { "pipeline_error" } else { "ok" },
        "action": request_action,
        "mode": mode.wire(),
        "request_artifacts": request_artifacts,
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

fn request_paths(project_root: &Path, request_id: &str) -> RequestPaths {
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

fn path_json(path: &Path) -> String {
    path.to_string_lossy().to_string()
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
        let paths = request_paths(Path::new("/tmp/project"), "req-abc");
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
        let paths = request_paths(Path::new("/repo"), "req-1");
        assert_eq!(
            paths.request,
            Path::new("/repo").join(".missiond/requests/req-1/request.lisp")
        );
        assert_eq!(
            paths.initial_event,
            Path::new("/repo").join(".missiond/requests/req-1/events/000001.event.lisp")
        );
    }
}
