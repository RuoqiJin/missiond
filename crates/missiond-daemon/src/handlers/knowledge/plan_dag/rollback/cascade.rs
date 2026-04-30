//! Conservative cascade rollback planning and dispatch.
//!
//! This module owns compensation discovery/order, plan-mode compensation
//! projection, and dispatch-safe cascade execution. The parent rollback.rs
//! keeps the shared policy/evaluation types and node-local rollback core.

use missiond_core::types::Plan;
use std::collections::{HashMap, HashSet};

use crate::state::AppState;

use super::{
    build_rollback_descriptor, truncate_rollback_brief_preview, CascadeCompensationOutcome,
    CascadeRollbackOutcome, DagNode, RollbackCascadeMode, RollbackDescriptor, RollbackPolicy,
    RollbackStatus,
};

/// wave-18 / task 04 — pure helper that computes the ordered list of
/// compensation node ids for a given failed (cascade-root) node.
///
/// Compensation discovery: every plan node carrying
/// `:compensates "<root_id>"` (case-insensitive on the root id) is a
/// candidate. Ordering rules:
///
///   1. Honour each candidate's `:rollback-after` hints when those
///      targets are also compensation candidates for the SAME root.
///      The cascade ordering uses a Kahn-style topological sort
///      restricted to compensation candidates.
///   2. Tie-break by the topological-sort `order` produced by
///      `build_validated_dag` so dispatch order in the cascade
///      mirrors the forward DAG.
///   3. If `:rollback-after` introduces a cycle (typo), fall back to
///      step (2)'s declaration order — we never deadlock the cascade
///      because a typo cannot be a fatal scheduler condition.
///
/// Pure: no IO, no AppState reads. Decoupled so the cascade evaluator
/// + unit tests can pin the contract identically.
pub(in crate::handlers::knowledge::plan_dag) fn compute_compensation_order<'a>(
    failed_id: &str,
    nodes: &'a [DagNode],
    forward_order: &[String],
) -> Vec<&'a DagNode> {
    let root_lc = failed_id.trim().to_ascii_lowercase();
    // wave-19 / task 10 — forward `:compensate-node` refs declared on
    // the failing node also surface compensation candidates. Build the
    // forward-ref id set (case-insensitive) by inspecting the failing
    // node's `compensate_node` slot. The validator (`build_validated_dag`)
    // has already rejected self-refs / unknown ids / disagreements
    // against any reverse `:compensates` declaration, so here we can
    // safely union the two directions without re-checking agreement.
    let forward_targets: HashSet<String> = nodes
        .iter()
        .filter(|n| n.id.to_ascii_lowercase() == root_lc)
        .filter_map(|n| {
            n.compensate_node
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase())
        })
        .collect();
    // Compensation candidate set + lookup helpers. A node is a candidate
    // iff EITHER it carries the reverse `:compensates "<root>"` declaration
    // OR the failing (root) node points at it via `:compensate-node`.
    let candidates: Vec<&DagNode> = nodes
        .iter()
        .filter(|n| {
            let reverse_match = n
                .compensates
                .as_deref()
                .map(|s| s.trim().to_ascii_lowercase() == root_lc)
                .unwrap_or(false);
            let forward_match = forward_targets.contains(&n.id.to_ascii_lowercase());
            reverse_match || forward_match
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    // Forward-order rank table — used as a tie-breaker so cascade
    // dispatch mirrors forward dispatch order when no `:rollback-after`
    // edge exists.
    let forward_rank: HashMap<&str, usize> = forward_order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    // Restrict the dependency graph to compensation candidates only.
    // `:rollback-after` edges that reference non-candidates are
    // dropped silently (they cannot block the cascade because the
    // referenced node will never run as a compensation step here).
    let candidate_ids: HashSet<&str> = candidates.iter().map(|n| n.id.as_str()).collect();
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &candidates {
        indeg.entry(n.id.as_str()).or_insert(0);
        for after in &n.rollback_after {
            let after_s = after.trim();
            if after_s.is_empty() || !candidate_ids.contains(after_s) {
                continue;
            }
            // Edge: after_s → n.id (n must run AFTER `after_s`).
            *indeg.entry(n.id.as_str()).or_insert(0) += 1;
            succs.entry(after_s).or_default().push(n.id.as_str());
        }
    }
    // Kahn-style topological sort with deterministic tie-break: at
    // each step we pop the candidate with the smallest forward-order
    // rank. Falls back to the declaration order embedded in
    // `candidates` for IDs not present in `forward_order`.
    let mut by_id: HashMap<&str, &DagNode> =
        candidates.iter().map(|n| (n.id.as_str(), *n)).collect();
    let declaration_rank: HashMap<&str, usize> = candidates
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let rank_of = |id: &str| -> (usize, usize) {
        (
            forward_rank.get(id).copied().unwrap_or(usize::MAX),
            declaration_rank.get(id).copied().unwrap_or(usize::MAX),
        )
    };
    let mut ordered: Vec<&DagNode> = Vec::with_capacity(candidates.len());
    loop {
        // Collect all currently-zero-indegree candidates.
        let mut ready: Vec<&str> = indeg
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(id, _)| *id)
            .collect();
        if ready.is_empty() {
            break;
        }
        ready.sort_by_key(|id| rank_of(id));
        // Pop the smallest-ranked entry.
        let next = ready[0];
        // Defensive: if for some reason the candidate is missing,
        // skip it.
        if let Some(n) = by_id.remove(next) {
            ordered.push(n);
        }
        indeg.remove(next);
        if let Some(children) = succs.remove(next) {
            for child in children {
                if let Some(d) = indeg.get_mut(child) {
                    *d = d.saturating_sub(1);
                }
            }
        }
    }
    // Cycle / unresolved entries — fall back to declaration order so
    // a typo never deadlocks the cascade.
    if ordered.len() < candidates.len() {
        let already: HashSet<&str> = ordered.iter().map(|n| n.id.as_str()).collect();
        for n in &candidates {
            if !already.contains(n.id.as_str()) {
                ordered.push(*n);
            }
        }
    }
    ordered
}

/// wave-18 / task 04 — pure helper that builds a `plan`-mode cascade
/// outcome for a single compensation node. Records intent + brief
/// preview but never dispatches. Decoupled so unit tests can pin the
/// shape without standing up an `AppState`.
pub(in crate::handlers::knowledge::plan_dag) fn build_compensation_plan_entry(
    plan: &Plan,
    node: &DagNode,
) -> CascadeCompensationOutcome {
    let descriptor = build_rollback_descriptor(node);
    let policy = descriptor.policy;
    let hints = descriptor.to_workstation_hints(node);
    let strategy = node.dispatch_strategy.as_deref().unwrap_or("unknown");
    let preview = if descriptor.objective.is_some() {
        Some(truncate_rollback_brief_preview(
            &super::super::super::workstation_dispatch::build_task_brief(plan, &hints, strategy),
        ))
    } else {
        None
    };
    CascadeCompensationOutcome {
        node_id: node.id.clone(),
        policy,
        status: RollbackStatus::DescriptorReady,
        reason: "cascade plan: compensation node recorded; no dispatch performed".to_string(),
        objective: descriptor.objective,
        owned_files: descriptor.owned_files,
        acceptance_commands: descriptor.acceptance_commands,
        task_brief_preview: preview,
        task_brief_path: None,
        inner_payload: None,
    }
}

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
                                    super::super::super::workstation_dispatch::run_workstation_dispatch(
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

/// wave-18 / task 04 — pure helper translating a workstation-dispatch
/// outcome into a single cascade compensation row. Decoupled from the
/// async cascade body so unit tests can pin every dispatch branch.
fn map_dispatch_outcome_to_compensation(
    node_id: String,
    descriptor: RollbackDescriptor,
    outcome: super::super::super::workstation_dispatch::WorkstationDispatchOutcome,
) -> CascadeCompensationOutcome {
    use super::super::super::workstation_dispatch::WorkstationDispatchOutcome as O;
    match outcome {
        O::Dispatched {
            task_brief,
            task_brief_path,
            inner_payload,
            ..
        } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Dispatched,
            reason:
                "cascade dispatch-safe: workstation dispatch completed; inner handler returned Ok"
                    .to_string(),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
            task_brief_path,
            inner_payload: Some(inner_payload),
        },
        O::DryRun { task_brief } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Dispatched,
            reason: "cascade dispatch-safe: substrate ran dry_run (no real handler invoked)"
                .to_string(),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
            task_brief_path: None,
            inner_payload: None,
        },
        O::InnerError {
            task_brief,
            inner_payload,
        } => {
            let detail = inner_payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("cascade compensation inner handler returned error")
                .to_string();
            CascadeCompensationOutcome {
                node_id,
                policy: RollbackPolicy::Workstation,
                status: RollbackStatus::Failed,
                reason: format!(
                    "cascade dispatch-safe: workstation dispatch failed: {}",
                    detail
                ),
                objective: descriptor.objective,
                owned_files: descriptor.owned_files,
                acceptance_commands: descriptor.acceptance_commands,
                task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                task_brief_path: None,
                inner_payload: Some(inner_payload),
            }
        }
        O::SafeDescriptor { reason, task_brief } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Refused,
            reason: format!(
                "cascade dispatch-safe refused (substrate): {}",
                reason.detail()
            ),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: task_brief.as_deref().map(truncate_rollback_brief_preview),
            task_brief_path: None,
            inner_payload: None,
        },
    }
}
