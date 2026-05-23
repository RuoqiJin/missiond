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

use crate::handlers::knowledge::file_artifacts::{
    ArtifactCommitEnvelope, ArtifactCommitEnvelopeInput,
};
use crate::state::AppState;

use super::request_artifacts::{
    build_artifact_existence, build_artifact_paths_json, nonblank, now_rfc3339, path_json,
    projection_to_json, request_paths_for, resolve_request_project_root, run_projection,
    sanitize_request_id, tool_result_payload, ProjectionStatus, RequestMode, RequestPaths,
};
use super::review_packet::{
    derive_request_projection, extract_mode_from_request_lisp, latest_review_event_checkpoint,
    read_artifact_existence, ReviewPacketInputs,
};
use super::{compat_write_requested, lisp_string, EVENT_SCHEMA};

pub(super) mod events;
pub(super) mod materialization;
pub(super) mod routing;

use self::events::{
    build_review_event_lisp, event_path_for_seq, list_event_filenames, next_action_for,
    next_event_seq, RespondOutcome, ReviewEventArgs,
};
use self::materialization::{
    board_task_materialization_to_json, ensure_request_board_task, materialize_request_plan,
    plan_materialization_to_json, BoardTaskMaterialization, PlanMaterialization,
};
use self::routing::{
    build_respond_plan_compile_args, extract_lisp_keyword_string, parse_respond_decision,
    resolve_directive_ref, resolve_plan_ref, DirectiveRef, PlanRef, RespondDecision,
};

// ───────────────────────────────────────────────────────────────────────
// review-response adapter — V3 unified-entry continuation. mission_request
// is the user-facing surface for answering a review_packet; it never
// silently approves, never bypasses mission_directive / mission_plan
// gates, and never spawns workstation work directly.
//
// Lisp authority:
//   .missiond/v3/missiond-blueprint.lisp :: unified-entry :: review-response
// ───────────────────────────────────────────────────────────────────────

pub(super) fn read_event_texts(events_dir: &Path, filenames: &[String]) -> Vec<String> {
    let mut names = filenames.to_vec();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| std::fs::read_to_string(events_dir.join(name)).ok())
        .collect()
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
            ));
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
            ));
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
    let event_write_outcome = ArtifactCommitEnvelope::commit_text(
        state,
        ArtifactCommitEnvelopeInput {
            operation_key: format!("mission_request:{request_id}:event:{allocated_seq:06}"),
            surface: "mission_request.respond".to_string(),
            request_id: Some(request_id.clone()),
            project_id: nonblank(args.get("project")),
            artifact_kind: "lifecycle-event".to_string(),
            artifact_path: event_path.clone(),
            content: event_body.clone(),
            overwrite: false,
            db_table: None,
            db_row_id: None,
            event_id: Some(format!("evt-{request_id}-{allocated_seq:06}")),
            event_seq: Some(allocated_seq as i64),
            payload: json!({
                "schema": EVENT_SCHEMA,
                "decision": decision.wire(),
                "outcome": outcome.wire(),
                "commit_surface": "mission_request.respond",
            }),
        },
    )
    .await;
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
            ));
        }
    };

    let existence = read_artifact_existence(&paths);
    let mut updated_event_texts = event_texts.clone();
    updated_event_texts.push(event_body.clone());
    let review_packet = derive_request_projection(
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
    )
    .to_review_packet_json();

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
