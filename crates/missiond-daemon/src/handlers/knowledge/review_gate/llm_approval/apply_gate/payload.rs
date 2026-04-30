use super::*;

/// Stamp the wave-22 / task 03 apply-gate outcome onto a response
/// payload under the stable `llm_approve_apply_gate` key. Pure / no
/// bus calls. Skipped when the gate was NotRequested so legacy callers
/// stay byte-identical with wave-21 / task 06.
pub(crate) fn stamp_llm_approve_apply_gate_payload(
    payload: &mut Value,
    outcome: &LlmApproveApplyGateOutcome,
) {
    if matches!(outcome.status, LlmApproveApplyStatus::NotRequested) {
        return;
    }
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert(
        "llm_approve_apply_gate".to_string(),
        outcome.to_response_json(),
    );
}

/// Augment the wave-21 / task 06 propose-only payload with the
/// deterministic proposal hash so callers can echo it back via
/// `proposal_hash` under `apply_llm_auto_approve=true` without having
/// to re-derive it themselves. Pure / no bus calls. Always runs
/// (regardless of mode) when a proposal is present so the wire shape
/// stays stable across runs.
pub(crate) fn stamp_proposal_hash_payload(
    payload: &mut Value,
    bundle: &LlmAutoApproveProposalBundle,
    action: &str,
    artifact_id: &str,
    version: i32,
) {
    let Some(p) = bundle.proposal.as_ref() else {
        return;
    };
    let hash = compute_proposal_hash(action, artifact_id, version, p);
    if let Some(map) = payload.as_object_mut() {
        map.insert("llm_auto_approve_proposal_hash".to_string(), json!(hash));
    }
}
