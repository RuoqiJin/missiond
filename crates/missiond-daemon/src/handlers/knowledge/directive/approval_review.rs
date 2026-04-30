use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::handlers::knowledge::review_gate::{
    build_llm_auto_approve_proposal_system_prompt, build_llm_auto_approve_proposal_user_prompt,
    enforce_apply_gate_preflight, enforce_proposal_invariants, evaluate_llm_approve_apply_gate,
    evaluate_review_automation, llm_auto_approve_proposal_mode_was_explicit,
    maybe_emit_review_question_resolved, parse_llm_approve_apply_gate_input,
    parse_llm_auto_approve_proposal, parse_llm_auto_approve_proposal_mode,
    parse_resolution_review_question_id, parse_review_automation_policy,
    parse_review_question_id_struct, parse_review_resolution_input, resolution_wire_string,
    review_automation_policy_was_explicit, stamp_llm_approve_apply_gate_payload,
    stamp_llm_auto_approve_proposal_payload, stamp_needs_changes_next_step,
    stamp_proposal_hash_payload, stamp_resolution_payload, stamp_review_automation_payload,
    validate_review_resolution_envelope, AutomationStatus, LlmApproveApplyGateInput,
    LlmAutoApproveProposalBundle, LlmAutoApproveProposalMode, LlmAutoApproveProposalStatus,
    ParsedReviewQuestionId, ResolutionOutcome, ReviewAutomationContext, ReviewAutomationPolicy,
    ReviewDecision, ReviewResolutionInput,
};
use crate::minimax_client::ChatMessage;
use crate::state::AppState;
use missiond_core::types::DirectiveStatus;

use super::{parse_id_arg, require_i32, SONNET_COMPILER_MODEL};

/// Action whitelist for the directive surface — the parsed
/// `review:directive:<id>:v<v>:<action>` envelope's `<action>` segment
/// must be in this list before we accept the resolution. We deliberately
/// scope to manager actions that change persisted state (compile / approve
/// / archive); `version_chain` / `get` / `list` never resolve a gate.
pub(super) const DIRECTIVE_REVIEW_ACTIONS: &[&str] = &["compile", "approve", "archive"];

/// Build the wave-18 / task 07 deterministic safety context for the
/// directive surface. Pure projection of the directive row + caller args.
///
/// Rules of thumb:
///   * `deterministic_mode` = `compiler_model.is_none()` (dry-run leaves
///     it unset; sonnet records `claude-sonnet`). LLM-driven artifacts
///     ALWAYS block `auto_safe` — refusing to auto-approve unattended
///     LLM output is the load-bearing safety contract.
///   * `protected_source_or_target` is currently `false` for directive —
///     directive surface has no merge source/target concept (that lives
///     on capability_usage). Future work: lift `protected_pillars`
///     from references_json once the capability protected list pivots
///     into the directive layer; for v0 we keep the rule loud-but-passing.
///   * File hashing flows through caller-supplied `expected_file_sha256`
///     (none today — directive compile never round-trips a hash through
///     approve/archive). The handler still honours it if a future caller
///     supplies the hint.
///   * `additional_blockers` carries any caller-side warnings the
///     handler already detected (currently empty — the wave-15 envelope
///     validators run BEFORE this, and any failure short-circuits the
///     whole resolution path before the policy fires).
fn build_directive_automation_ctx(
    args: &Value,
    directive_compiler_model: Option<&str>,
) -> ReviewAutomationContext {
    ReviewAutomationContext {
        deterministic_mode: directive_compiler_model.is_none(),
        // Directive approve/archive never re-touches the file artifact —
        // the wave-14 file-first writer ran during compile. So no file
        // write is "attempted" at the resolution boundary.
        file_write_attempted: false,
        file_write_succeeded: false,
        actual_file_sha256: None,
        expected_file_sha256: args
            .get("expected_file_sha256")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        protected_source_or_target: false,
        additional_blockers: Vec::new(),
    }
}

mod approve;
mod archive;
mod proposer;
mod subscriber;

pub(super) use self::approve::action_approve;
pub(super) use self::archive::action_archive;
use self::proposer::{
    attach_directive_apply_gate_block, attach_directive_proposal_block, directive_proposer_summary,
    parse_proposer_mode_or_error, request_directive_auto_approve_proposal,
};
pub(crate) use self::subscriber::{handle_review_resolved_event, DirectiveSubscriberOutcome};
