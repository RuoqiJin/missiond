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
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::fmt::Write as _;
#[cfg(test)]
use std::path::{Path, PathBuf};

use crate::handlers::knowledge::file_artifacts::atomic_write_artifact;
use crate::state::AppState;

mod request_artifacts;
mod respond;
mod review_packet;

#[cfg(test)]
use missiond_mcp::tools::ToolContent;
use request_artifacts::{
    build_artifact_existence, build_artifact_paths_json, build_event_lisp, build_request_lisp,
    extract_projected_sexp, nonblank, now_rfc3339, parse_mode, path_json, projection_to_json,
    request_id_from_args, request_paths_for, resolve_request_project_root, run_projection,
    sanitize_request_id, tool_result_payload, ProjectionOutcome, RequestDoc, RequestMode,
    RequestPaths,
};
#[cfg(test)]
use request_artifacts::{
    build_artifact_existence_with, classify_projection_target, extract_pipeline_meta,
    plan_projection, PipelineMeta, ProjectionPlan, ProjectionStatus, ProjectionTarget,
};
#[cfg(test)]
use respond::events::parse_event_seq_from_filename;
use respond::{action_respond, list_event_filenames, read_event_texts};
#[cfg(test)]
use respond::{
    build_respond_plan_compile_args, build_review_event_lisp, enrich_materialized_plan_lisp,
    event_path_for_seq, extract_directive_ref_from_artifact, extract_lisp_keyword_int,
    extract_lisp_keyword_string, next_action_for, next_event_seq, parse_respond_decision,
    plan_materialization_to_json, resolve_directive_ref, resolve_plan_ref, DirectiveRef,
    PlanArtifactProjection, PlanMaterialization, PlanRef, RespondDecision, RespondOutcome,
    RespondParseError, ReviewEventArgs,
};
#[cfg(test)]
use review_packet::{
    allowed_responses_for, build_review_artifact_preview, classify_review_state, ArtifactExistence,
    ReviewEventCheckpoint, ReviewState, REVIEW_PREVIEW_MAX_BYTES,
};
use review_packet::{
    derive_review_packet, extract_mode_from_request_lisp, latest_review_event_checkpoint,
    parse_execute_requested, read_artifact_existence, ReviewPacketInputs,
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
