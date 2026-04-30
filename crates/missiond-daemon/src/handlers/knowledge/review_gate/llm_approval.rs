//! LLM review proposal and explicit apply-gate helpers for review_gate.
//!
//! Kept as a facade so the parent review_gate.rs surface maps cleanly to V3
//! while proposal parsing and apply-gate evaluation live on separate function
//! boundaries. The load-bearing invariant is unchanged: proposal helpers never
//! auto-approve without the explicit apply gate.

mod apply_gate;
mod proposal;

#[allow(unused_imports)]
pub(crate) use apply_gate::{
    compute_proposal_hash, enforce_apply_gate_preflight, evaluate_llm_approve_apply_gate,
    parse_llm_approve_apply_gate_input, stamp_llm_approve_apply_gate_payload,
    stamp_proposal_hash_payload, LlmApproveApplyGateInput, LlmApproveApplyGateOutcome,
    LlmApproveApplyStatus, ProposalHashStatus, APPLY_GATE_INVALID_PARAM,
    APPLY_GATE_MISSING_PROPOSAL_HASH, APPLY_GATE_PROPOSAL_HASH_MISMATCH,
};
#[allow(unused_imports)]
pub(crate) use proposal::{
    build_llm_auto_approve_proposal_system_prompt, build_llm_auto_approve_proposal_user_prompt,
    enforce_proposal_invariants, llm_auto_approve_proposal_mode_was_explicit,
    parse_llm_auto_approve_proposal, parse_llm_auto_approve_proposal_mode,
    stamp_llm_auto_approve_proposal_payload, LlmAutoApproveProposal, LlmAutoApproveProposalBundle,
    LlmAutoApproveProposalConfidence, LlmAutoApproveProposalMode, LlmAutoApproveProposalStatus,
};
#[allow(unused_imports)]
pub(in crate::handlers::knowledge::review_gate) use proposal::{
    proposal_json_kind, strip_proposal_code_fence,
};
