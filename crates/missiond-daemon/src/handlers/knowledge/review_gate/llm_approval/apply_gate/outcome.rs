use super::*;

/// Pure outcome of [`evaluate_llm_approve_apply_gate`]. Side-effect free
/// — no DB, no bus, no LLM. The handler consumes this projection to
/// decide whether to run the existing `directive_approve` /
/// `plan_update_status(Approved)` transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LlmApproveApplyGateOutcome {
    /// Whether the caller opted into the gate at all. Echoes
    /// `LlmApproveApplyGateInput::apply` for audit symmetry.
    pub requested: bool,
    /// Wire status — the load-bearing signal for observers.
    pub status: LlmApproveApplyStatus,
    /// The decision the gate would have applied (always `Approved` when
    /// `status=Applied`; carries the proposal's decision under SKIP
    /// statuses so dashboards see what was offered). `None` when no
    /// proposal exists.
    pub applied_decision: Option<ReviewDecision>,
    /// Result of the proposal-hash comparison.
    pub proposal_hash_status: ProposalHashStatus,
    /// Hash the gate computed from the bundle (always populated when a
    /// proposal exists; None under Unavailable / NoSuggestion).
    pub computed_proposal_hash: Option<String>,
    /// Caller-supplied hash (echoed for audit symmetry).
    pub supplied_proposal_hash: Option<String>,
    /// Whether the caller flipped `caller_approved=true`.
    pub caller_approved: bool,
    /// Flat list of `code:detail` strings explaining every gate's
    /// outcome. Always populated under non-NotRequested statuses; the
    /// NotRequested status returns an empty vec so the response can
    /// omit the gate block entirely without losing audit detail.
    pub safety_rule_results: Vec<String>,
}

impl LlmApproveApplyGateOutcome {
    /// Build the wire shape consumed by the response payload. Always
    /// emits every field (with `null` for absent values) so observers
    /// can pivot on a stable shape regardless of which skip reason
    /// fired.
    pub(crate) fn to_response_json(&self) -> Value {
        json!({
            "requested": self.requested,
            "apply_status": self.status.as_str(),
            "applied_decision": self.applied_decision.map(|d| d.as_str()),
            "proposal_hash_status": self.proposal_hash_status.as_str(),
            "computed_proposal_hash": self.computed_proposal_hash.clone(),
            "supplied_proposal_hash": self.supplied_proposal_hash.clone(),
            "caller_approved": self.caller_approved,
            "safety_rule_results": self.safety_rule_results,
        })
    }
}
