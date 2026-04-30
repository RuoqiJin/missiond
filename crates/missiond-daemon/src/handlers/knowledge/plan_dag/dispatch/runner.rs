use anyhow::Result;
use missiond_core::types::Plan;
use serde_json::json;

use crate::state::AppState;

use super::super::super::super::compute::{flow_run, task_delegate};
use super::super::super::{agent_execution, plan, workstation_dispatch};
use super::super::scheduler::build_node_inner_args;
use super::super::DagNode;
use super::{
    task_contract_ctx::TaskContractDispatchCtx,
    types::DispatchOutcome,
    workstation::{node_to_workstation_hints, workstation_outcome_to_dispatch_pair},
};

pub(in crate::handlers::knowledge::plan_dag) async fn dispatch_node(
    state: AppState,
    plan: Plan,
    node: DagNode,
    task_contract_ctx: TaskContractDispatchCtx,
) -> Result<DispatchOutcome> {
    let inner_args_built = build_node_inner_args(&node, &plan);
    let dispatch_strategy = inner_args_built.dispatch_strategy.clone();

    // wave-15 / task 05 + wave-16 / task 03 — workstation-dispatch routing
    // for DAG nodes. Wave-15 honoured an explicit per-node
    // `:workstation-dispatch true` only. Wave-16 layers conservative
    // auto-inference on top: when a node's :target is already
    // `mission_task_delegate`, the dispatch strategy resolves to a known
    // workstation strategy, the objective is non-empty, and at least one
    // scoping signal is declared, the scheduler routes through the
    // workstation substrate without requiring the explicit hint. There is
    // no per-node `workstation_dispatch=false` knob because DAG nodes are
    // declared in PLAN.lisp; the only off-switch is to mark the node with
    // a non-task-delegate target or omit the dispatch strategy.
    let merged = node_to_workstation_hints(&node);
    let inference_ctx = workstation_dispatch::InferenceContext {
        target: node.target.as_str(),
        dispatch_strategy: dispatch_strategy.as_str(),
        objective: merged.objective.as_deref(),
        owned_files_present: !merged.owned_files.is_empty(),
        scope_present: merged
            .scope
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        target_project_present: merged
            .target_project
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        requested_cwd_present: merged
            .requested_cwd
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
    };
    let dispatch_decision = workstation_dispatch::evaluate_dispatch_decision(
        &serde_json::Value::Null,
        node.workstation_dispatch_opt_in(),
        &inference_ctx,
    );

    if dispatch_decision.is_enabled() {
        // wave-19 / task 06 — emit the per-node task-contract sidecar
        // BEFORE handing the node to the workstation substrate. The
        // contract is the SSOT, so a failed write REFUSES dispatch
        // for this node; non-retryable so the wave loop does not loop
        // through the inner handler hoping the disk recovers. Default
        // mode (`Off`) returns an empty record and the per-node
        // payload omits the wave-19 fields entirely.
        let inputs =
            plan::task_contract_inputs_from_hints(&merged, &node.target, &dispatch_strategy);
        let emission = plan::emit_task_contract(
            &state,
            plan.id,
            &plan.board_task_id,
            &node.id,
            task_contract_ctx.mode,
            &inputs,
            task_contract_ctx.project_arg.as_deref(),
            task_contract_ctx.cwd_arg.as_deref(),
            task_contract_ctx.target_project_arg.as_deref(),
        )
        .await;

        if emission.is_failure() {
            // Refuse the per-node dispatch — the missing contract
            // would leave downstream consumers with no Lisp SSOT.
            // Mark non-retryable: an IO failure is unlikely to fix
            // itself by re-running the inner handler.
            let mut payload = json!({
                "node_id": node.id,
                "target": node.target,
                "workstation_dispatch_status": "skipped_task_contract_emit_failed",
                "workstation_dispatch_source": dispatch_decision.source.as_str(),
            });
            if let Some(reason) = dispatch_decision.reason.as_deref() {
                payload["workstation_dispatch_inference_reason"] = json!(reason);
            }
            plan::merge_task_contract_block(&mut payload, &emission);
            let reason = emission
                .error
                .clone()
                .unwrap_or_else(|| "task_contract_emit_failed".to_string());
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: payload,
                classification: Err(format!(
                    "task_contract emit failed for node `{}`: {}",
                    node.id, reason
                )),
                non_retryable: true,
            });
        }

        if task_contract_ctx.mode.is_dry_run() {
            // EmitDryRun — never call the substrate. We mark the
            // node succeeded (the contract write IS the work in
            // dry-run mode); downstream nodes proceed normally so
            // the caller can preview the full DAG with one pass.
            let mut payload = json!({
                "node_id": node.id,
                "target": node.target,
                "workstation_dispatch_status": "task_contract_emit_dry_run",
                "workstation_dispatch_source": dispatch_decision.source.as_str(),
            });
            if let Some(reason) = dispatch_decision.reason.as_deref() {
                payload["workstation_dispatch_inference_reason"] = json!(reason);
            }
            plan::merge_task_contract_block(&mut payload, &emission);
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: payload,
                classification: Ok(()),
                non_retryable: false,
            });
        }

        // wave-20 / task 04 — when the per-DAG-run dispatch_contract_mode
        // is `machine` AND emission produced a contract path for THIS
        // node, hand the on-disk Lisp to the wave-19 / task 07
        // consumer. The consumer overlays the contract onto the
        // hints (contract is the SSOT) and refuses to fall back to
        // the legacy natural-language brief on a malformed contract
        // (surfacing as `SafeDescriptor` →
        // status="skipped_malformed_task_contract", non-retryable
        // because re-loading a syntactically broken file deterministically
        // fails again). Default mode (`rendered`) preserves wave-15..19
        // byte-shape: `task_contract_path = None` and the brief is
        // built from the in-memory hints.
        let task_contract_path_for_machine =
            if task_contract_ctx.dispatch_contract_mode.is_machine() {
                emission.path.clone()
            } else {
                None
            };
        let outcome = workstation_dispatch::run_workstation_dispatch_with_contract(
            &state,
            &plan,
            &node.target,
            &dispatch_strategy,
            merged,
            false,
            task_contract_path_for_machine.as_deref(),
        )
        .await;
        let (mut inner_payload, classification, non_retryable) =
            workstation_outcome_to_dispatch_pair(
                &node,
                &dispatch_strategy,
                outcome,
                &dispatch_decision,
            );
        // wave-20 / task 04 — surface the resolved dispatch-contract
        // mode so observers (PR review, CI, audit) can pin which
        // dispatch contract drove the brief at this node. The wire
        // shape adds one new key per node — existing callers that
        // ignore it keep working.
        if let Some(map) = inner_payload.as_object_mut() {
            map.insert(
                "dispatch_contract_mode".to_string(),
                json!(task_contract_ctx.dispatch_contract_mode.as_str()),
            );
        }
        plan::merge_task_contract_block(&mut inner_payload, &emission);
        return Ok(DispatchOutcome {
            node_id: node.id.clone(),
            target: node.target.clone(),
            dispatch_strategy,
            inner_payload,
            classification,
            non_retryable,
        });
    }

    let inner_args = match inner_args_built.inner_args {
        Ok(v) => v,
        Err(err_payload) => {
            let reason = err_payload
                .as_object()
                .and_then(|m| m.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("inner args build failed")
                .to_string();
            // wave-16 / task 05 — inner-args build failures are deterministic
            // (e.g. missing required `flow_id` for `mission_flow_run`).
            // Re-running with identical inputs would fail identically;
            // mark non-retryable so the wave loop skips the retry pass.
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: err_payload,
                classification: Err(reason),
                non_retryable: true,
            });
        }
    };

    let inner_result = match node.target.as_str() {
        "mission_execution" => {
            agent_execution::handle(&state, "mission_execution", inner_args.clone()).await?
        }
        "mission_task_delegate" => {
            task_delegate::handle(&state, "mission_task_delegate", inner_args.clone()).await?
        }
        "mission_flow_run" => {
            flow_run::handle(&state, "mission_flow_run", inner_args.clone()).await?
        }
        _ => unreachable!("DAG validation already enforced target whitelist"),
    };

    let inner_payload = plan::tool_result_payload(&inner_result);
    let inner_is_error = inner_result.is_error.unwrap_or(false);
    let classification = if inner_is_error {
        Err(inner_payload
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("inner handler returned error")
            .to_string())
    } else {
        Ok(())
    };
    Ok(DispatchOutcome {
        node_id: node.id.clone(),
        target: node.target.clone(),
        dispatch_strategy,
        inner_payload,
        classification,
        // Standard inner-handler failures may have transient causes —
        // leave them retryable. The wave loop honours the per-node
        // retry policy and stops once attempts are exhausted.
        non_retryable: false,
    })
}
