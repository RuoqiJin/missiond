use super::*;

// ───────────────────────────────────────────────────────────────────────
// wave-21 / task 06 — LLM auto-approve proposal v0 (directive surface)
//
// Conservative wiring on top of the existing wave-15 / 18 / 20 stack.
// The new `auto_approve_mode` knob is OPT-IN (default `off`); when
// `sonnet_suggest` is supplied we invoke Sonnet for a propose-only
// review-action recommendation and surface it under
// `llm_auto_approve_proposal*` fields on the response payload. The
// proposal NEVER drives a DB transition or bus emission in v0 —
// `applied=false` is pinned on every payload (invariant I3) and
// `requires_human=true` is forced regardless of model output.
//
// Destructive actions (`archive`) ALWAYS short-circuit to
// `destructive_blocked`: the proposal value is preserved for audit but
// the response carries `requires_human=true` AND a loud warning. This
// matches the wave-18 / 07 archive-never-auto-promoted contract for the
// deterministic policy path.
//
// Sonnet unavailable surfaces `LLM_UNAVAILABLE` status with no fallback
// proposal — invariant I4 forbids silent degradation to deterministic.
// ───────────────────────────────────────────────────────────────────────

const DIRECTIVE_REVIEW_PROPOSER_CALLER: &str = "directive_review_proposer";
const SONNET_PROPOSER_MAX_TOKENS: u32 = 1024;

/// Run the wave-21 / task 06 propose-only Sonnet pass for the directive
/// surface. Returns a fully-built [`LlmAutoApproveProposalBundle`] in
/// every code path so the caller can pivot on the bundle status without
/// branching on Result. NEVER mutates state.
pub(super) async fn request_directive_auto_approve_proposal(
    state: &AppState,
    mode: LlmAutoApproveProposalMode,
    action: &str,
    artifact_id: &uuid::Uuid,
    version: i32,
    deterministic_summary: &Value,
    artifact_digest: Option<&str>,
) -> LlmAutoApproveProposalBundle {
    // Invariant I2 short-circuit — destructive actions never drive a
    // Sonnet call in v0. We surface `destructive_blocked` directly so
    // dashboards can grep for the refusal without reading per-handler
    // state.
    if crate::handlers::knowledge::review_gate::is_destructive_review_action(action) {
        return LlmAutoApproveProposalBundle::destructive_blocked(
            mode,
            action,
            DIRECTIVE_REVIEW_PROPOSER_CALLER,
            None,
            format!(
                "rule:destructive_action:`{}` is destructive; auto-approve proposal NEVER promotes (invariant I2)",
                action.to_ascii_lowercase()
            ),
        );
    }

    // Invariant I4 — Sonnet unavailable surfaces `Unavailable` with no
    // fallback proposal. We mirror the directive_compile dry-run rule
    // here for consistency with the rest of the file.
    let Some(sonnet) = state.sonnet.as_ref() else {
        return LlmAutoApproveProposalBundle::unavailable(
            mode,
            action,
            DIRECTIVE_REVIEW_PROPOSER_CALLER,
            "Sonnet gateway not initialized; LLM auto-approve proposal unavailable",
        );
    };

    let system = build_llm_auto_approve_proposal_system_prompt();
    let user = build_llm_auto_approve_proposal_user_prompt(
        "directive",
        action,
        &artifact_id.to_string(),
        version,
        deterministic_summary,
        artifact_digest,
    );
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ];

    let raw = match sonnet
        .call_interactive(
            messages,
            Some(SONNET_PROPOSER_MAX_TOKENS),
            DIRECTIVE_REVIEW_PROPOSER_CALLER,
        )
        .await
    {
        Ok(s) => s,
        Err(err) => {
            return LlmAutoApproveProposalBundle::unavailable(
                mode,
                action,
                DIRECTIVE_REVIEW_PROPOSER_CALLER,
                format!("Sonnet auto-approve proposal call failed: {}", err),
            );
        }
    };

    let (proposal, parse_warnings) = parse_llm_auto_approve_proposal(&raw);
    match proposal {
        Some(mut p) => {
            // Pin the deterministic invariants (destructive_check +
            // requires_human always true in v0).
            enforce_proposal_invariants(&mut p, action);
            LlmAutoApproveProposalBundle {
                mode,
                status: LlmAutoApproveProposalStatus::Suggested,
                proposal: Some(p),
                proposal_warnings: parse_warnings,
                unavailable_reason: None,
                action: action.to_string(),
                request_caller: Some(DIRECTIVE_REVIEW_PROPOSER_CALLER.to_string()),
                model: Some(SONNET_COMPILER_MODEL.to_string()),
            }
        }
        None => LlmAutoApproveProposalBundle {
            mode,
            status: LlmAutoApproveProposalStatus::NoSuggestion,
            proposal: None,
            proposal_warnings: parse_warnings,
            unavailable_reason: None,
            action: action.to_string(),
            request_caller: Some(DIRECTIVE_REVIEW_PROPOSER_CALLER.to_string()),
            model: Some(SONNET_COMPILER_MODEL.to_string()),
        },
    }
}

/// Splice the wave-21 / task 06 bundle onto a response payload. Skips
/// the splice when the bundle is `not_invoked` (`mode=off`) so legacy
/// callers stay byte-identical with pre-wave-21.
pub(super) fn attach_directive_proposal_block(
    payload: &mut Value,
    bundle: &LlmAutoApproveProposalBundle,
) {
    if matches!(bundle.status, LlmAutoApproveProposalStatus::NotInvoked) {
        return;
    }
    stamp_llm_auto_approve_proposal_payload(payload, bundle);
}

/// Wave-22 / task 03 :: stamp the proposal hash + apply-gate outcome
/// onto the response payload. Pure / no DB mutation. The hash is
/// always stamped when the bundle carries a proposal (so callers can
/// echo it back via `proposal_hash` under `apply_llm_auto_approve=true`
/// without re-deriving it themselves). The apply-gate block is only
/// stamped when the gate was requested (preserving wave-21 byte-shape).
pub(super) fn attach_directive_apply_gate_block(
    payload: &mut Value,
    bundle: &LlmAutoApproveProposalBundle,
    input: &LlmApproveApplyGateInput,
    artifact_id: &uuid::Uuid,
    version: i32,
) -> crate::handlers::knowledge::review_gate::LlmApproveApplyGateOutcome {
    stamp_proposal_hash_payload(
        payload,
        bundle,
        &bundle.action,
        &artifact_id.to_string(),
        version,
    );
    let outcome = evaluate_llm_approve_apply_gate(
        input,
        bundle,
        &bundle.action,
        &artifact_id.to_string(),
        version,
    );
    stamp_llm_approve_apply_gate_payload(payload, &outcome);
    outcome
}

/// Read + validate the wave-21 / task 06 mode arg. Returns
/// `Ok(None)` when the caller did not opt in (mode=off OR field absent),
/// `Ok(Some(mode))` when sonnet_suggest is requested. Strict-enum: typo
/// values fail-fast with a structured error so callers never silently
/// degrade to off.
pub(super) fn parse_proposer_mode_or_error(
    args: &Value,
) -> std::result::Result<Option<LlmAutoApproveProposalMode>, ToolError> {
    let mode = parse_llm_auto_approve_proposal_mode(args)
        .map_err(|msg| ToolError::new(error_codes::INVALID_PARAM, msg))?;
    if mode.is_sonnet_suggest() {
        Ok(Some(mode))
    } else {
        // Echo `Off` only when the caller explicitly supplied it (not
        // when the field was absent) so dashboards can tell the
        // difference between "caller opted out" and "caller never saw
        // the knob". For absent / off, we omit the bundle entirely to
        // preserve byte-shape.
        if llm_auto_approve_proposal_mode_was_explicit(args) {
            Ok(Some(mode))
        } else {
            Ok(None)
        }
    }
}

/// Build the deterministic summary block that feeds into the Sonnet
/// prompt. Tiny / pure / no I/O. Only the fields the model actually
/// needs surface here so the prompt stays terse.
pub(super) fn directive_proposer_summary(
    automation_outcome_status: &str,
    automation_policy: &str,
    decision_present: bool,
) -> Value {
    json!({
        "review_automation_policy": automation_policy,
        "review_automation_status": automation_outcome_status,
        "explicit_decision_supplied": decision_present,
    })
}
