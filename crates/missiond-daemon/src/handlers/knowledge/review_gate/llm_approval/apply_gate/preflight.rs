use super::*;

/// Strict pre-flight for the wave-22 / task 03 apply gate. Runs the
/// fail-fast hash-missing / hash-mismatch checks BEFORE any state
/// mutation. Returns `Ok(())` when:
///   * caller did not opt in (`apply=false`);
///   * caller opted in AND supplied a hash that matches.
/// Returns `Err((code, message))` for the two contract-mandated
/// structured errors:
///   * `APPLY_GATE_MISSING_PROPOSAL_HASH` — `apply=true` without a hash.
///   * `APPLY_GATE_PROPOSAL_HASH_MISMATCH` — `apply=true` with a hash
///     that does not match the bundle.
///
/// The handler converts the Err into [`ToolResult::structured_error`]
/// BEFORE running the existing `directive_approve` /
/// `plan_update_status` transition, satisfying the contract: "On
/// mismatch or missing proposal hash, return structured error and do
/// not mutate directive/plan/review state."
pub(crate) fn enforce_apply_gate_preflight(
    input: &LlmApproveApplyGateInput,
    bundle: &LlmAutoApproveProposalBundle,
    action: &str,
    artifact_id: &str,
    version: i32,
) -> std::result::Result<(), (String, String)> {
    if !input.apply {
        return Ok(());
    }
    // Without a proposal we cannot compute a hash; structured-error so
    // the caller knows to retry under `auto_approve_mode=sonnet_suggest`
    // first (or to drop the apply flag).
    let proposal = match bundle.proposal.as_ref() {
        Some(p) => p,
        None => {
            // Missing hash also implicitly fails for no-proposal bundles
            // — surface the more specific message so the caller knows
            // the proposal is the missing piece.
            if input.proposal_hash.is_none() {
                return Err((
                    APPLY_GATE_MISSING_PROPOSAL_HASH.to_string(),
                    format!(
                        "apply_llm_auto_approve=true requires `proposal_hash` AND a Sonnet proposal to apply against; bundle status `{}` carries no proposal",
                        bundle.status.as_str(),
                    ),
                ));
            }
            // Caller supplied a hash but the bundle has no proposal —
            // hash cannot match. Surface mismatch so dashboards see the
            // load-bearing reason.
            return Err((
                APPLY_GATE_PROPOSAL_HASH_MISMATCH.to_string(),
                format!(
                    "apply_llm_auto_approve=true with `proposal_hash` but bundle status `{}` carries no proposal to compare against",
                    bundle.status.as_str(),
                ),
            ));
        }
    };
    let hash = compute_proposal_hash(action, artifact_id, version, proposal);
    match input.proposal_hash.as_deref() {
        None => Err((
            APPLY_GATE_MISSING_PROPOSAL_HASH.to_string(),
            format!(
                "apply_llm_auto_approve=true requires `proposal_hash`; expected `{}` (echoed under `llm_auto_approve_proposal_hash` in the propose-only response)",
                hash,
            ),
        )),
        Some(s) if s.eq_ignore_ascii_case(&hash) => Ok(()),
        Some(s) => Err((
            APPLY_GATE_PROPOSAL_HASH_MISMATCH.to_string(),
            format!(
                "apply_llm_auto_approve=true with `proposal_hash=`{}`` does not match bundle hash `{}` (action=`{}` artifact=`{}` v{})",
                s, hash, action, artifact_id, version,
            ),
        )),
    }
}
