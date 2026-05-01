use super::*;

/// wave-18 / task 07 :: build the deterministic safety context for a
/// plan-side resolution. Mirrors the directive helper:
///   * `deterministic_mode` = `compiler_model.is_none()` (dry-run leaves
///     it unset; sonnet records the V3-projected compiler model). LLM-driven plans
///     always block `auto_safe`.
///   * `protected_source_or_target` is currently `false` — plan rows
///     have no merge source/target concept; the rule still records a
///     loud-but-passing reason.
///   * Caller may opt into hash matching via `expected_file_sha256`
///     (none today; the wave-14 file-first writer surfaces the actual
///     hash on compile, and a future caller can pass the captured value
///     here).
pub(super) fn build_plan_automation_ctx(
    args: &Value,
    plan_compiler_model: Option<&str>,
) -> ReviewAutomationContext {
    ReviewAutomationContext {
        deterministic_mode: plan_compiler_model.is_none(),
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

// ───────────────────────────────────────────────────────────────────────
// wave-21 / task 06 — LLM auto-approve proposal v0 (plan surface)
//
// Mirrors the directive surface — see directive.rs comment for the full
// invariant table. Wired into approve / mark / supersede; supersede is
// destructive and ALWAYS short-circuits to `destructive_blocked`. The
// proposal NEVER drives a DB transition or bus emission in v0
// (`applied=false` pinned; `requires_human=true` forced).
// ───────────────────────────────────────────────────────────────────────

const PLAN_REVIEW_PROPOSER_CALLER: &str = "plan_review_proposer";
const SONNET_PLAN_PROPOSER_MAX_TOKENS: u32 = 1024;

pub(super) async fn request_plan_auto_approve_proposal(
    state: &AppState,
    mode: LlmAutoApproveProposalMode,
    action: &str,
    artifact_id: &uuid::Uuid,
    version: i32,
    deterministic_summary: &Value,
    artifact_digest: Option<&str>,
) -> LlmAutoApproveProposalBundle {
    if crate::handlers::knowledge::review_gate::is_destructive_review_action(action) {
        return LlmAutoApproveProposalBundle::destructive_blocked(
            mode,
            action,
            PLAN_REVIEW_PROPOSER_CALLER,
            None,
            format!(
                "rule:destructive_action:`{}` is destructive; auto-approve proposal NEVER promotes (invariant I2)",
                action.to_ascii_lowercase()
            ),
        );
    }

    let Some(sonnet) = state.sonnet.as_ref() else {
        return LlmAutoApproveProposalBundle::unavailable(
            mode,
            action,
            PLAN_REVIEW_PROPOSER_CALLER,
            "Sonnet gateway not initialized; LLM auto-approve proposal unavailable",
        );
    };

    let system = build_llm_auto_approve_proposal_system_prompt();
    let user = build_llm_auto_approve_proposal_user_prompt(
        "plan",
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
            Some(SONNET_PLAN_PROPOSER_MAX_TOKENS),
            PLAN_REVIEW_PROPOSER_CALLER,
        )
        .await
    {
        Ok(s) => s,
        Err(err) => {
            return LlmAutoApproveProposalBundle::unavailable(
                mode,
                action,
                PLAN_REVIEW_PROPOSER_CALLER,
                format!("Sonnet auto-approve proposal call failed: {}", err),
            );
        }
    };

    let (proposal, parse_warnings) = parse_llm_auto_approve_proposal(&raw);
    let compiler_model = match load_sonnet_compiler_model() {
        Ok(model) => model,
        Err(err) => {
            return LlmAutoApproveProposalBundle::unavailable(
                mode,
                action,
                PLAN_REVIEW_PROPOSER_CALLER,
                err.to_string(),
            );
        }
    };
    match proposal {
        Some(mut p) => {
            enforce_proposal_invariants(&mut p, action);
            LlmAutoApproveProposalBundle {
                mode,
                status: LlmAutoApproveProposalStatus::Suggested,
                proposal: Some(p),
                proposal_warnings: parse_warnings,
                unavailable_reason: None,
                action: action.to_string(),
                request_caller: Some(PLAN_REVIEW_PROPOSER_CALLER.to_string()),
                model: Some(compiler_model.clone()),
            }
        }
        None => LlmAutoApproveProposalBundle {
            mode,
            status: LlmAutoApproveProposalStatus::NoSuggestion,
            proposal: None,
            proposal_warnings: parse_warnings,
            unavailable_reason: None,
            action: action.to_string(),
            request_caller: Some(PLAN_REVIEW_PROPOSER_CALLER.to_string()),
            model: Some(compiler_model),
        },
    }
}

pub(super) fn attach_plan_proposal_block(
    payload: &mut Value,
    bundle: &LlmAutoApproveProposalBundle,
) {
    if matches!(bundle.status, LlmAutoApproveProposalStatus::NotInvoked) {
        return;
    }
    stamp_llm_auto_approve_proposal_payload(payload, bundle);
}

/// Wave-22 / task 03 :: stamp the proposal hash + apply-gate outcome
/// onto the plan response payload. Pure / no DB mutation. Mirrors
/// `attach_directive_apply_gate_block` from directive.rs — see that
/// helper for the design rationale.
pub(super) fn attach_plan_apply_gate_block(
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

pub(super) fn parse_plan_proposer_mode_or_error(
    args: &Value,
) -> std::result::Result<Option<LlmAutoApproveProposalMode>, ToolError> {
    let mode = parse_llm_auto_approve_proposal_mode(args)
        .map_err(|msg| ToolError::new(error_codes::INVALID_PARAM, msg))?;
    if mode.is_sonnet_suggest() {
        Ok(Some(mode))
    } else if llm_auto_approve_proposal_mode_was_explicit(args) {
        Ok(Some(mode))
    } else {
        Ok(None)
    }
}

pub(super) fn plan_proposer_summary(
    automation_outcome_status: &str,
    automation_policy: &str,
    decision_present: bool,
    extra: Option<(&str, Value)>,
) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "review_automation_policy".to_string(),
        json!(automation_policy),
    );
    map.insert(
        "review_automation_status".to_string(),
        json!(automation_outcome_status),
    );
    map.insert(
        "explicit_decision_supplied".to_string(),
        json!(decision_present),
    );
    if let Some((k, v)) = extra {
        map.insert(k.to_string(), v);
    }
    Value::Object(map)
}
