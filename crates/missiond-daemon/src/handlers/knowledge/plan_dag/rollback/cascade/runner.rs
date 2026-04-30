use missiond_core::types::Plan;

use crate::state::AppState;

use super::super::{
    build_rollback_descriptor, CascadeCompensationOutcome, CascadeRollbackOutcome, DagNode,
    RollbackCascadeMode, RollbackPolicy, RollbackStatus,
};
use super::{
    dispatch_outcome::map_dispatch_outcome_to_compensation, ordering::compute_compensation_order,
    plan_entry::build_compensation_plan_entry,
};

/// wave-18 / task 04 — async cascade evaluator. Runs AFTER a node's
/// final failed attempt and AFTER the node-local `run_rollback`. Pure
/// when `mode == Plan` (no IO beyond `build_task_brief`); only the
/// `DispatchSafe` mode invokes the substrate.
///
/// Behaviour matrix:
///
///   * `mode == None`         — returns an inactive outcome; the wave
///                              loop suppresses the cascade surface.
///   * `mode == Plan`         — every compensation node lands as
///                              `descriptor_ready`. **Never dispatches.**
///   * `mode == DispatchSafe` — for each compensation node, run the
///                              wave-17 / task 04 safety check on its
///                              own descriptor; only dispatch when the
///                              gate passes AND the compensation node's
///                              policy is `workstation`. Any safety /
///                              substrate refusal lands as `refused`
///                              (non-retryable). `descriptor`-only
///                              compensations stay `descriptor_ready`.
pub(in crate::handlers::knowledge::plan_dag) async fn run_cascade_rollback(
    state: &AppState,
    plan: &Plan,
    failed_node: &DagNode,
    nodes: &[DagNode],
    forward_order: &[String],
) -> CascadeRollbackOutcome {
    let mode = failed_node
        .rollback_cascade_kind()
        .unwrap_or(RollbackCascadeMode::None);
    if matches!(mode, RollbackCascadeMode::None) {
        return CascadeRollbackOutcome {
            mode,
            cascade_root: failed_node.id.clone(),
            compensations: Vec::new(),
            reason: "cascade rollback not requested".to_string(),
        };
    }
    let ordered = compute_compensation_order(&failed_node.id, nodes, forward_order);
    if ordered.is_empty() {
        return CascadeRollbackOutcome {
            mode,
            cascade_root: failed_node.id.clone(),
            compensations: Vec::new(),
            reason: format!(
                "cascade {}: no compensation nodes declared `:compensates \"{}\"`",
                mode.as_wire(),
                failed_node.id
            ),
        };
    }
    let mut compensations: Vec<CascadeCompensationOutcome> = Vec::with_capacity(ordered.len());
    for n in ordered {
        match mode {
            RollbackCascadeMode::None => unreachable!(),
            RollbackCascadeMode::Plan => {
                compensations.push(build_compensation_plan_entry(plan, n));
            }
            RollbackCascadeMode::DispatchSafe => {
                // Only dispatch when the compensation node's own
                // rollback policy is `workstation` AND every safety
                // gate passes. Otherwise fall back to `plan` mode for
                // this entry — record intent, never dispatch.
                let descriptor = build_rollback_descriptor(n);
                match descriptor.policy {
                    RollbackPolicy::Workstation => {
                        match descriptor.safety_check_for_workstation(n) {
                            Err(reason) => {
                                // Safety gate refused — record refusal,
                                // never retry.
                                compensations.push(CascadeCompensationOutcome {
                                    node_id: n.id.clone(),
                                    policy: RollbackPolicy::Workstation,
                                    status: RollbackStatus::Refused,
                                    reason: format!("cascade dispatch-safe refused: {}", reason),
                                    objective: descriptor.objective,
                                    owned_files: descriptor.owned_files,
                                    acceptance_commands: descriptor.acceptance_commands,
                                    task_brief_preview: None,
                                    task_brief_path: None,
                                    inner_payload: None,
                                });
                            }
                            Ok(()) => {
                                let hints = descriptor.to_workstation_hints(n);
                                let strategy = n.dispatch_strategy.as_deref().unwrap_or("unknown");
                                let outcome =
                                    super::super::super::super::workstation_dispatch::run_workstation_dispatch(
                                        state,
                                        plan,
                                        "mission_task_delegate",
                                        strategy,
                                        hints,
                                        false,
                                    )
                                    .await;
                                compensations.push(map_dispatch_outcome_to_compensation(
                                    n.id.clone(),
                                    descriptor,
                                    outcome,
                                ));
                            }
                        }
                    }
                    RollbackPolicy::Descriptor | RollbackPolicy::None => {
                        // Compensation node opted into descriptor-only
                        // (or no rollback policy at all). Cascade
                        // dispatch-safe MUST NEVER promote a non-
                        // workstation compensation to a dispatch — that
                        // would silently change the scope of work the
                        // author authorised. Record the plan entry and
                        // move on.
                        compensations.push(build_compensation_plan_entry(plan, n));
                    }
                }
            }
        }
    }
    let dispatched = compensations
        .iter()
        .filter(|c| matches!(c.status, RollbackStatus::Dispatched))
        .count();
    let refused = compensations
        .iter()
        .filter(|c| matches!(c.status, RollbackStatus::Refused))
        .count();
    let failed = compensations
        .iter()
        .filter(|c| matches!(c.status, RollbackStatus::Failed))
        .count();
    let recorded = compensations
        .iter()
        .filter(|c| matches!(c.status, RollbackStatus::DescriptorReady))
        .count();
    let reason = format!(
        "cascade {}: compensation_nodes={} recorded={} dispatched={} refused={} failed={}",
        mode.as_wire(),
        compensations.len(),
        recorded,
        dispatched,
        refused,
        failed,
    );
    CascadeRollbackOutcome {
        mode,
        cascade_root: failed_node.id.clone(),
        compensations,
        reason,
    }
}
