use super::*;

/// Pure deterministic SHA-256 hash over the LOAD-BEARING fields of a
/// proposal. Truncated to the leading 32 hex chars (128 bits — way more
/// than enough collision resistance for an audit-trail correlator).
///
/// Inputs: action label, artifact id, artifact version, the proposal's
/// decision wire string, the proposal's confidence wire string, and the
/// proposal's deterministic destructive_check substring (we use the
/// "destructive:" / "non_destructive:" prefix only, not the full free
/// text, so the hash stays stable across superficial wording changes).
///
/// The hash is what the caller is expected to echo back via the
/// `proposal_hash` arg under `apply_llm_auto_approve=true`. Caller can
/// derive it themselves from the proposal block — we surface the same
/// value under `llm_auto_approve_proposal_hash` so dashboards can
/// `assert hash == derive(...)` directly.
pub(crate) fn compute_proposal_hash(
    action: &str,
    artifact_id: &str,
    version: i32,
    proposal: &LlmAutoApproveProposal,
) -> String {
    use sha2::{Digest, Sha256};
    // We hash a CANONICAL serialisation: lower-case action; trimmed
    // artifact id; ascii integer version; decision wire; confidence
    // wire; the destructive_check prefix (everything before the first
    // colon, lowercased). Any other proposal field (evidence /
    // non_goal_check) is intentionally OUT of the hash so superficial
    // text differences don't churn the audit correlator.
    let action_norm = action.trim().to_ascii_lowercase();
    let artifact_norm = artifact_id.trim();
    let destructive_prefix = proposal
        .destructive_check
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let payload = format!(
        "v1|{}|{}|{}|{}|{}|{}",
        action_norm,
        artifact_norm,
        version,
        proposal.decision.as_str(),
        proposal.confidence.as_str(),
        destructive_prefix,
    );
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    let full = format!("{:x}", h.finalize());
    full.chars().take(32).collect()
}
