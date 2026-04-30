use anyhow::Result;
use missiond_core::types::{Plan, PlanStatus};
use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};

use crate::handlers::knowledge::review_gate::{
    parse_review_question_id_struct, PlanNodeResumeInput, ReviewDecision,
};
use crate::state::AppState;

use super::super::{
    build_validated_dag, dispatch_node, emit_evidence_finished, emit_evidence_running,
    DispatchOutcome, EvidenceCtx, ExecutionOutcome, TaskContractDispatchCtx,
    PLAN_NODE_DEFAULT_ATTEMPT,
};
use super::evidence::emit_resume_decision_evidence;
use super::validation::{validate_resume_request, PlanNodeResumeError};

/// Wave-17 / task 01 — single-node resume entrypoint invoked from
/// `mission_plan(action=execute)` when the caller supplies the
/// `resume_review_question_id` field set. Performs the validate /
/// dispatch / evidence cycle for ONE paused node and surfaces the
/// outcome on the response payload.
///
/// Only the targeted node is touched. Downstream nodes that were
/// pending after the original paused dispatch stay pending — the
/// caller is expected to drive a follow-up `mission_plan(execute)`
/// call to advance them. This conservative scope matches the wave-17
/// brief: "Only resume existing paused node state. No broad PLAN
/// reinterpretation."
pub(in crate::handlers::knowledge) async fn action_execute_resume(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    input: PlanNodeResumeInput,
) -> Result<ToolResult> {
    if !matches!(plan.status, PlanStatus::Approved | PlanStatus::Executing) {
        return Ok(resume_error_to_tool_result(
            PlanNodeResumeError::PlanStatusNotExecutable {
                status: plan.status.as_str().to_string(),
            },
        ));
    }

    let parsed_qid = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(resume_error_to_tool_result(
                PlanNodeResumeError::IdMalformed {
                    detail: e.message(),
                },
            ))
        }
    };

    let (parsed_dag, _order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => {
            return Ok(resume_error_to_tool_result(
                PlanNodeResumeError::DagBuildFailed {
                    detail: format!("{:?}", e),
                },
            ))
        }
    };

    let node = match validate_resume_request(&parsed_qid, plan, &parsed_dag) {
        Ok(n) => n.clone(),
        Err(e) => return Ok(resume_error_to_tool_result(e)),
    };

    let dispatch_strategy = node
        .dispatch_strategy
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let ctx = EvidenceCtx {
        plan_id: plan.id,
        plan_version: plan.version,
        project_arg: args.get("project").and_then(|v| v.as_str()),
        cwd_arg: args.get("cwd").and_then(|v| v.as_str()),
        target_project_arg: args.get("target_project").and_then(|v| v.as_str()),
    };

    // Evidence is recorded for EVERY decision (approved/rejected/
    // needs_changes) so the audit trail captures the resume even when
    // we refuse to dispatch.
    let mut outcome = ExecutionOutcome::default();
    let resume_event_ref = emit_resume_decision_evidence(
        state,
        &ctx,
        plan,
        &node,
        &dispatch_strategy,
        &input,
        &mut outcome,
    )
    .await;

    let mut payload = json!({
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "node_id": node.id,
        "review_question_id": input.question_id,
        "review_decision": input.decision.as_str(),
        "review_resume": true,
    });
    if let Some(actor) = input.actor.as_deref() {
        payload["resume_actor"] = json!(actor);
    }
    if let Some(note) = input.note.as_deref() {
        payload["resume_note"] = json!(note);
    }
    if let Some(ref_event_id) = resume_event_ref.as_deref() {
        payload["resume_event_id"] = json!(ref_event_id);
    }

    match input.decision {
        ReviewDecision::Approved => {
            // Fresh attempt 1: paused is non-terminal (no failed
            // attempt was consumed by the gate emit), so the resume
            // dispatch is conceptually a brand-new run of the node.
            let attempt = PLAN_NODE_DEFAULT_ATTEMPT;
            emit_evidence_running(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                attempt,
                &mut outcome,
            )
            .await;
            // wave-19 / task 06 — resume path also honours the
            // task-contract knobs from the resume call. We re-build
            // the ctx from `args` because the paused-node code path
            // never threaded the original DAG run's ctx through —
            // and that's the right semantic: the resume request is a
            // fresh execute call so it gets to set its own contract
            // policy.
            let resume_task_contract_ctx = match TaskContractDispatchCtx::from_args(args) {
                Ok(c) => c,
                Err(err) => return Ok(err),
            };
            let dispatch_outcome = match dispatch_node(
                state.clone(),
                plan.clone(),
                node.clone(),
                resume_task_contract_ctx,
            )
            .await
            {
                Ok(o) => o,
                Err(e) => {
                    return Err(e);
                }
            };
            let DispatchOutcome {
                node_id: _,
                target,
                dispatch_strategy: ds,
                inner_payload,
                classification,
                non_retryable: _,
            } = dispatch_outcome;
            let succeeded = classification.is_ok();
            emit_evidence_finished(
                state,
                &ctx,
                &node,
                &ds,
                &inner_payload,
                succeeded,
                attempt,
                &mut outcome,
            )
            .await;
            payload["status"] = json!(if succeeded {
                "resume_dispatched"
            } else {
                "resume_failed"
            });
            payload["target"] = json!(target);
            payload["dispatch_strategy"] = json!(ds);
            payload["inner_result"] = inner_payload;
            payload["attempt"] = json!(attempt);
            if let Err(reason) = classification {
                payload["reason"] = json!(reason);
            }
        }
        ReviewDecision::Rejected => {
            payload["status"] = json!("resume_rejected");
            payload["target"] = json!(node.target);
            payload["dispatch_strategy"] = json!(dispatch_strategy);
            payload["next_step"] = json!(format!(
                "node `{}` remains paused; recompile the plan or supply \
                 review_decision=approved to dispatch it",
                node.id
            ));
        }
        ReviewDecision::NeedsChanges => {
            payload["status"] = json!("resume_needs_changes");
            payload["target"] = json!(node.target);
            payload["dispatch_strategy"] = json!(dispatch_strategy);
            payload["next_step"] = json!(format!(
                "rework node `{}` per `resume_note`, recompile the plan, \
                 then resume against the new node-hash",
                node.id
            ));
        }
    }

    if let Some(p) = outcome.evidence_path.as_deref() {
        payload["evidence_path"] = json!(p);
    }
    if let Some(e) = outcome.evidence_error.as_deref() {
        payload["evidence_error"] = json!(e);
    }
    if !outcome.bus_publish_warnings.is_empty() {
        payload["bus_publish_warnings"] = json!(outcome.bus_publish_warnings);
    }

    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-17 / task 01 — convert a resume validation failure into the
/// canonical `ToolResult::structured_error` shape so the
/// `mission_plan(action=execute)` boundary always surfaces actionable
/// error codes.
fn resume_error_to_tool_result(err: PlanNodeResumeError) -> ToolResult {
    ToolResult::structured_error(ToolError::new(err.code(), err.message()))
}
