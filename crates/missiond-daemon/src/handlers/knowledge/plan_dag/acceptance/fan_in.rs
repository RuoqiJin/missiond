use std::collections::HashMap;

use super::super::{DagNode, NodeResult, NodeState};
use super::payload::inner_payload_missing_keys;
use super::types::{
    AcceptanceEvaluation, AcceptanceFanInOutcome, AcceptanceRequires, AcceptanceStatus,
};

/// wave-18 / task 03 — apply cross-node acceptance fan-in on top of the
/// per-node evaluation. Pure: never touches the bus, never executes
/// shell, only inspects the prior nodes' terminal lifecycle state and
/// recorded `inner_payload`. Runs AFTER `evaluate_node_acceptance`; the
/// per-node status acts as a precondition:
///
///   * `NotEvaluated` (no per-node hints) — fan-in still runs because
///     `:acceptance-depends-on` is itself an opt-in. Pass flips status
///     to `Accepted`; fail flips it to `Rejected`.
///   * `Accepted`     — fan-in pass keeps `Accepted`; fail flips to
///                       `Rejected`.
///   * `Rejected` / `ManualRequired` — the per-node decision dominates.
///                       Fan-in is recorded for audit but does NOT
///                       override the parent decision (we don't promote
///                       a rejected node to accepted, and we don't
///                       de-pause a manual_required node).
///
/// `prior_results` is the scheduler's `results_by_id` snapshot keyed by
/// node id; each entry's `state` and `inner_payload` are the source of
/// truth. Missing source entries (which the validator forbids at build
/// time) collapse to a fan-in failure with a loud reason — defence in
/// depth in case the scheduler ever calls this without the validator.
pub(in crate::handlers::knowledge::plan_dag) fn apply_acceptance_fan_in(
    base: AcceptanceEvaluation,
    node: &DagNode,
    prior_results: &HashMap<String, &NodeResult>,
) -> AcceptanceEvaluation {
    if !node.has_acceptance_fan_in() {
        return base;
    }
    // SAFETY: `has_acceptance_fan_in` already proved both halves are
    // present + recognised, so the unwraps below cannot fire.
    let mode = node.acceptance_requires_kind().expect(
        "has_acceptance_fan_in implies acceptance_requires_kind() is Some — \
         validator must have raised earlier",
    );
    let source_nodes: Vec<String> = node.acceptance_depends_on.clone();

    // Per-node evaluation must dominate when it already produced a
    // terminal "do not accept" signal. We still record the fan-in for
    // audit so observers can see what the gate would have decided.
    let parent_dominates = matches!(
        base.status,
        AcceptanceStatus::Rejected | AcceptanceStatus::ManualRequired
    );

    let outcome = match mode {
        AcceptanceRequires::AllSucceeded => {
            let mut failing: Vec<String> = Vec::new();
            for id in &source_nodes {
                let succeeded = prior_results
                    .get(id)
                    .map(|r| matches!(r.state, NodeState::Succeeded))
                    .unwrap_or(false);
                if !succeeded {
                    failing.push(id.clone());
                }
            }
            if failing.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: true,
                    reason: format!(
                        "all_succeeded: every source node ({}) reached succeeded",
                        source_nodes.len()
                    ),
                }
            } else {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: format!("all_succeeded: source node(s) not succeeded: {:?}", failing),
                }
            }
        }
        AcceptanceRequires::AnySucceeded => {
            let mut succeeded_any = false;
            for id in &source_nodes {
                if let Some(r) = prior_results.get(id) {
                    if matches!(r.state, NodeState::Succeeded) {
                        succeeded_any = true;
                        break;
                    }
                }
            }
            if succeeded_any {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: true,
                    reason: "any_succeeded: at least one source node reached succeeded".to_string(),
                }
            } else {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: format!(
                        "any_succeeded: no source node ({}) reached succeeded",
                        source_nodes.len()
                    ),
                }
            }
        }
        AcceptanceRequires::EvidenceKeys => {
            // Validator guarantees `acceptance_source_node` is set AND
            // present in `acceptance_depends_on` AND in the plan, but
            // we defend in depth — a missing entry fails the gate
            // loudly instead of silently passing.
            let source_id = node.acceptance_source_node.clone().unwrap_or_default();
            let single_source = vec![source_id.clone()];
            let keys = super::super::super::plan::split_lisp_string_list(
                node.acceptance_evidence_keys_raw.as_deref(),
            );
            if source_id.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: "evidence_keys: :acceptance-source-node is missing".to_string(),
                }
            } else if keys.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: single_source,
                    passed: false,
                    reason: "evidence_keys: :acceptance-evidence-keys is empty — nothing to prove"
                        .to_string(),
                }
            } else {
                match prior_results.get(&source_id) {
                    None => AcceptanceFanInOutcome {
                        mode,
                        source_nodes: single_source,
                        passed: false,
                        reason: format!(
                            "evidence_keys: source node `{}` produced no result",
                            source_id
                        ),
                    },
                    Some(r) => {
                        let missing = inner_payload_missing_keys(&r.inner_payload, &keys);
                        if missing.is_empty() {
                            AcceptanceFanInOutcome {
                                mode,
                                source_nodes: single_source,
                                passed: true,
                                reason: format!(
                                    "evidence_keys: source node `{}` carries every required key",
                                    source_id
                                ),
                            }
                        } else {
                            AcceptanceFanInOutcome {
                                mode,
                                source_nodes: single_source,
                                passed: false,
                                reason: format!(
                                    "evidence_keys: source node `{}` missing keys {:?}",
                                    source_id, missing
                                ),
                            }
                        }
                    }
                }
            }
        }
    };

    let mut next = base;
    let fan_in_passed = outcome.passed;
    let fan_in_reason = outcome.reason.clone();
    next.fan_in = Some(outcome);

    if parent_dominates {
        // Per-node decision wins; fan-in is informational only.
        return next;
    }

    if fan_in_passed {
        // NotEvaluated → Accepted, Accepted → Accepted (status stable).
        if matches!(next.status, AcceptanceStatus::NotEvaluated) {
            next.status = AcceptanceStatus::Accepted;
            next.reason = format!("acceptance_fan_in: {}", fan_in_reason);
        }
    } else {
        next.status = AcceptanceStatus::Rejected;
        next.reason = format!("acceptance_fan_in: {}", fan_in_reason);
    }
    next
}
