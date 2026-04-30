use serde_json::{json, Value};

use super::super::super::{plan, workstation_dispatch};
use super::super::DagNode;

/// Project a parsed DAG node into the workstation-dispatch hint contract.
/// Mirrors `ParsedPlanHints::to_workstation_hints` so the v0 DAG path and
/// the v0 single-node runner build identical briefs for the same hints.
pub(in crate::handlers::knowledge::plan_dag) fn node_to_workstation_hints(
    node: &DagNode,
) -> workstation_dispatch::WorkstationDispatchHints {
    workstation_dispatch::WorkstationDispatchHints {
        objective: node.objective.clone(),
        scope: node.scope.clone(),
        owned_files: plan::split_lisp_string_list(node.owned_files_raw.as_deref()),
        forbidden_files: plan::split_lisp_string_list(node.forbidden_files_raw.as_deref()),
        acceptance_commands: plan::split_lisp_string_list(node.acceptance_commands_raw.as_deref()),
        commit_policy: node.commit_policy.clone(),
        target_project: node.target_project.clone(),
        requested_cwd: node.requested_cwd.clone(),
        dispatch_strategy: node.dispatch_strategy.clone(),
    }
}

/// Convert a workstation-dispatch outcome into the
/// `(inner_payload, classification, non_retryable)` triple `dispatch_node`
/// uses to populate `DispatchOutcome`. Keeps the per-node DAG contract
/// intact: the response JSON carries the workstation-dispatch envelope
/// under `inner_result`, and the outcome's status drives the
/// success/failure classification.
///
/// wave-16 / task 05 — `non_retryable` is true ONLY for
/// `SafeDescriptor` outcomes, because those refusals are deterministic
/// policy checks (unsupported target / project root unresolved /
/// missing objective). Re-running the same inputs would refuse
/// identically; the scheduler respects this and bypasses the retry
/// loop. `InnerError` (the substrate handler returned an error
/// payload) IS retryable — that path may have transient causes.
pub(in crate::handlers::knowledge::plan_dag) fn workstation_outcome_to_dispatch_pair(
    node: &DagNode,
    dispatch_strategy: &str,
    outcome: workstation_dispatch::WorkstationDispatchOutcome,
    decision: &workstation_dispatch::DispatchDecision,
) -> (Value, std::result::Result<(), String>, bool) {
    let status = outcome.status();
    let envelope = workstation_dispatch::outcome_to_response_fields(&outcome, dispatch_strategy);
    let mut non_retryable = false;
    let classification: std::result::Result<(), String> = match &outcome {
        workstation_dispatch::WorkstationDispatchOutcome::Dispatched { .. } => Ok(()),
        workstation_dispatch::WorkstationDispatchOutcome::DryRun { .. } => Ok(()),
        workstation_dispatch::WorkstationDispatchOutcome::InnerError { inner_payload, .. } => {
            Err(inner_payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("workstation_dispatch inner handler returned error")
                .to_string())
        }
        workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor { reason, .. } => {
            // Safe-descriptor refusals are deterministic policy checks
            // — flag the failure as non-retryable so the wave loop
            // skips the retry pass entirely.
            non_retryable = true;
            Err(format!(
                "workstation_dispatch refused to dispatch node `{}`: {}",
                node.id,
                reason.detail()
            ))
        }
    };
    let mut payload = json!({
        "workstation_dispatch_status": status,
        "node_id": node.id,
        // wave-16 / task 03 — surface routing provenance per node so the
        // DAG response makes the explicit/inferred split visible without
        // re-deriving from the plan body.
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        if let Some(map) = payload.as_object_mut() {
            map.insert(
                "workstation_dispatch_inference_reason".to_string(),
                json!(reason),
            );
        }
    }
    if let Some(map) = envelope.as_object() {
        if let Some(payload_map) = payload.as_object_mut() {
            for (k, v) in map {
                payload_map.insert(k.clone(), v.clone());
            }
        }
    }
    (payload, classification, non_retryable)
}
