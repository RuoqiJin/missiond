use serde_json::Value;

use super::super::DagNode;
use super::payload::{inner_payload_failure_signal, inner_payload_missing_keys};
use super::types::{AcceptanceEvaluation, AcceptanceMode, AcceptanceStatus};

/// wave-17 / task 03 — pure deterministic acceptance evaluator. NEVER
/// runs shell. Decides one of the four [`AcceptanceStatus`] values based
/// on the node's hints + the inner dispatch payload.
///
/// Decision tree (in order):
///   1. No hints at all (no mode + no commands + no keys) →
///      `NotEvaluated`. The caller preserves the wave-13
///      succeed-on-dispatch contract.
///   2. Mode = `Manual` → `ManualRequired` (always pauses, regardless of
///      payload). Reason: `"manual mode declared"`.
///   3. Mode = `InnerStatus` → `Accepted` iff `dispatch_succeeded` AND
///      the inner payload does not carry an explicit failure status
///      (`success=false`, `error` string, or `status="error"`).
///      Otherwise `Rejected` with a reason explaining the mismatch.
///   4. Mode = `EvidenceKeys` → `Accepted` iff every required key is
///      present in the inner payload's typed-evidence projection;
///      otherwise `Rejected` with the missing-key list. Empty key list
///      degrades to `ManualRequired` (an empty contract cannot prove
///      anything).
///   5. Mode unset but `:acceptance-commands` declared → `ManualRequired`
///      (we refuse to run shell). Reason captures the command count so
///      observers can tell why the gate triggered.
///   6. Otherwise (mode unset, no commands, only stray keys) →
///      `ManualRequired` so the author's typo surfaces loudly.
///
/// `dispatch_succeeded` is the boolean we already computed from the
/// inner classification. The evaluator never re-derives it from the
/// payload — that would risk drifting from the dispatch judgment.
pub(in crate::handlers::knowledge::plan_dag) fn evaluate_node_acceptance(
    node: &DagNode,
    inner_payload: &Value,
    dispatch_succeeded: bool,
) -> AcceptanceEvaluation {
    let commands =
        super::super::super::plan::split_lisp_string_list(node.acceptance_commands_raw.as_deref());
    let evidence_keys = super::super::super::plan::split_lisp_string_list(
        node.acceptance_evidence_keys_raw.as_deref(),
    );
    let mode_raw = node.acceptance_mode_raw.as_deref().unwrap_or("").trim();
    let mode = if mode_raw.is_empty() {
        None
    } else {
        AcceptanceMode::parse(mode_raw)
    };

    if mode.is_none() && commands.is_empty() && evidence_keys.is_empty() {
        return AcceptanceEvaluation {
            status: AcceptanceStatus::NotEvaluated,
            mode: None,
            commands,
            evidence_keys,
            reason: "no acceptance hints declared".to_string(),
            fan_in: None,
        };
    }

    // wave-18 / task 03 — when the node opted into cross-node fan-in
    // AND did NOT declare a per-node `:acceptance-mode`, the
    // `:acceptance-evidence-keys` list is owned by the fan-in
    // evaluator (its `evidence_keys` mode reads them off the source
    // node's payload). Surface a `NotEvaluated` per-node decision in
    // that case so `apply_acceptance_fan_in` is the sole decider; the
    // wave-17 "keys without mode → manual_required" warning would
    // otherwise pre-empt fan-in.
    if mode.is_none() && commands.is_empty() && node.has_acceptance_fan_in() {
        return AcceptanceEvaluation {
            status: AcceptanceStatus::NotEvaluated,
            mode: None,
            commands,
            evidence_keys,
            reason: "per-node acceptance deferred to cross-node fan-in evaluator".to_string(),
            fan_in: None,
        };
    }

    match mode {
        Some(AcceptanceMode::Manual) => AcceptanceEvaluation {
            status: AcceptanceStatus::ManualRequired,
            mode,
            commands,
            evidence_keys,
            reason: "acceptance-mode=manual; human approval required".to_string(),
            fan_in: None,
        },
        Some(AcceptanceMode::InnerStatus) => {
            if !dispatch_succeeded {
                return AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "inner_status: dispatch classification was not Ok".to_string(),
                    fan_in: None,
                };
            }
            if let Some(detail) = inner_payload_failure_signal(inner_payload) {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: format!(
                        "inner_status: inner payload reports non-success ({})",
                        detail
                    ),
                    fan_in: None,
                }
            } else {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Accepted,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "inner_status: dispatch Ok and payload carries no error signal"
                        .to_string(),
                    fan_in: None,
                }
            }
        }
        Some(AcceptanceMode::EvidenceKeys) => {
            if evidence_keys.is_empty() {
                return AcceptanceEvaluation {
                    status: AcceptanceStatus::ManualRequired,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "evidence_keys mode declared but :acceptance-evidence-keys is empty"
                        .to_string(),
                    fan_in: None,
                };
            }
            let missing = inner_payload_missing_keys(inner_payload, &evidence_keys);
            if missing.is_empty() {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Accepted,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "evidence_keys: all required keys present in inner payload".to_string(),
                    fan_in: None,
                }
            } else {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: format!("evidence_keys: missing required keys {:?}", missing),
                    fan_in: None,
                }
            }
        }
        None => {
            // Mode unset but the author declared SOME acceptance hint.
            // We refuse to execute shell from PLAN.lisp, so the only
            // safe default is to surface the gate as manual_required.
            let reason = if !commands.is_empty() {
                format!(
                    "acceptance commands declared ({} item(s)) without :acceptance-mode; \
                     PLAN DAG never runs shell — manual approval required",
                    commands.len()
                )
            } else {
                format!(
                    "acceptance evidence keys declared ({} item(s)) without :acceptance-mode; \
                     manual approval required",
                    evidence_keys.len()
                )
            };
            AcceptanceEvaluation {
                status: AcceptanceStatus::ManualRequired,
                mode,
                commands,
                evidence_keys,
                reason,
                fan_in: None,
            }
        }
    }
}
