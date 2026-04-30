use super::*;

mod subscriber;

pub(crate) use self::subscriber::{handle_review_resolved_event, PlanSubscriberOutcome};

// ───────────────────────────────────────────────────────────────────────
// approve / mark / supersede — control actions
// ───────────────────────────────────────────────────────────────────────

/// Action whitelist for the plan surface — the parsed
/// `review:plan:<id>:v<v>:<action>` envelope's `<action>` segment must be
/// in this list before we accept the resolution. Mirrors the manager
/// state-changing actions: compile / approve / mark / supersede. (`get`
/// / `list` / `by_task` / `record_evidence` / `execute` never resolve a
/// gate.)
pub(super) const PLAN_REVIEW_ACTIONS: &[&str] = &["compile", "approve", "mark", "supersede"];

/// wave-18 / task 07 :: build the deterministic safety context for a
/// plan-side resolution. Mirrors the directive helper:
///   * `deterministic_mode` = `compiler_model.is_none()` (dry-run leaves
///     it unset; sonnet records `claude-sonnet`). LLM-driven plans
///     always block `auto_safe`.
///   * `protected_source_or_target` is currently `false` — plan rows
///     have no merge source/target concept; the rule still records a
///     loud-but-passing reason.
///   * Caller may opt into hash matching via `expected_file_sha256`
///     (none today; the wave-14 file-first writer surfaces the actual
///     hash on compile, and a future caller can pass the captured value
///     here).
fn build_plan_automation_ctx(
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

async fn request_plan_auto_approve_proposal(
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
            request_caller: Some(PLAN_REVIEW_PROPOSER_CALLER.to_string()),
            model: Some(SONNET_COMPILER_MODEL.to_string()),
        },
    }
}

fn attach_plan_proposal_block(payload: &mut Value, bundle: &LlmAutoApproveProposalBundle) {
    if matches!(bundle.status, LlmAutoApproveProposalStatus::NotInvoked) {
        return;
    }
    stamp_llm_auto_approve_proposal_payload(payload, bundle);
}

/// Wave-22 / task 03 :: stamp the proposal hash + apply-gate outcome
/// onto the plan response payload. Pure / no DB mutation. Mirrors
/// `attach_directive_apply_gate_block` from directive.rs — see that
/// helper for the design rationale.
fn attach_plan_apply_gate_block(
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

fn parse_plan_proposer_mode_or_error(
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

fn plan_proposer_summary(
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

pub(super) async fn action_approve(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front so caller typos surface as INVALID_PARAM BEFORE any
    // DB read.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. Strict
    // shape errors fail-fast as INVALID_PARAM BEFORE any DB read.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge. When the caller supplies
    // `review_question_id` + `review_decision` we validate the envelope
    // BEFORE mutating plan state. `Rejected` / `NeedsChanges` skip the
    // approve transition entirely; `Approved` proceeds with the existing
    // `plan_update_status(Approved)` call.
    //
    // wave-18 / task 07 :: when a non-Manual `review_automation_policy`
    // is supplied without an explicit `review_decision` (which would
    // otherwise fail-fast with MISSING_PARAM), promote the qid into a
    // policy-driven evaluation path. Caller-supplied decisions ALWAYS
    // win over the policy.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(
                    e,
                    crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision
                )
            {
                return Ok(ToolResult::structured_error(ToolError::new(
                    e.code(),
                    e.message(),
                )));
            }
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return plan_action_approve_with_policy_only(
                state,
                id,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_approve_with_resolution(
            state,
            id,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    // wave-22 / task 03 :: when caller opted into the apply gate, the
    // legacy unconditional `plan_update_status(Approved)` is INVERTED —
    // the DB transition is gated on the LLM proposal passing all 6
    // strict gates. See directive.rs::action_approve for the full
    // design rationale (mirrored here for the plan surface).
    if apply_gate_input.apply {
        // We need the current plan version so the proposal hash is
        // computed against the head. Source it from the store.
        let plan = match state
            .store
            .plan_get(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
        {
            Some(p) => p,
            None => {
                return Ok(ToolResult::structured_error(ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("plan `{}` not found for apply gate", id),
                )))
            }
        };
        let resolved_mode = proposer_mode.unwrap_or(LlmAutoApproveProposalMode::Off);
        let summary = plan_proposer_summary("legacy_quiet", "manual", false, None);
        let bundle = request_plan_auto_approve_proposal(
            state,
            resolved_mode,
            "approve",
            &id,
            plan.version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        if let Err((code, msg)) = enforce_apply_gate_preflight(
            &apply_gate_input,
            &bundle,
            "approve",
            &id.to_string(),
            plan.version,
        ) {
            return Ok(ToolResult::structured_error(ToolError::new(code, msg)));
        }
        let mut payload = json!({
            "plan_id": id,
            "version": plan.version,
        });
        attach_plan_proposal_block(&mut payload, &bundle);
        let outcome = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            plan.version,
        );
        if outcome.status.should_apply() {
            state
                .store
                .plan_update_status(id, PlanStatus::Approved)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
            payload["resolution_source"] = json!("llm_approve_apply_gate");
            let qid = parse_resolution_review_question_id(args);
            maybe_emit_review_question_resolved(
                &mut payload,
                &state.bus,
                qid.as_deref(),
                "approved",
                None,
            )
            .await;
        } else {
            payload["status"] = json!("llm_auto_apply_skipped");
            payload["next_step"] = json!(format!(
                "apply gate did not authorise (status={}); supply explicit `review_decision=approved` to flip the plan manually OR re-run with a matching proposal_hash + caller_approved=true",
                outcome.status.as_str()
            ));
        }
        return Ok(ToolResult::json_pretty(&payload));
    }

    state
        .store
        .plan_update_status(id, PlanStatus::Approved)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "approved",
        "plan_id": id,
    });
    // wave-11/14 quiet emit path — kept for callers that fire a Resolved
    // event without the wave-15 decision-bearing envelope.
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(&mut payload, &state.bus, qid.as_deref(), "approved", None)
        .await;
    // wave-21 / task 06 :: propose-only Sonnet pass on the legacy path.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary("legacy_quiet", "manual", false, None);
        let bundle =
            request_plan_auto_approve_proposal(state, mode, "approve", &id, 0, &summary, None)
                .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: stamp the proposal hash so callers can
        // echo it back via `proposal_hash` under
        // `apply_llm_auto_approve=true` on a follow-up call.
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", &id.to_string(), 0);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=approve`. Validates the
/// review envelope (scope / artifact / version / action) against the
/// current plan row, then performs the manager transition only when the
/// decision is `approved`.
///
/// wave-18 / task 07 :: also evaluates the deterministic
/// `review_automation_policy` and stamps the suggestion / status onto
/// the response payload. Caller-supplied `review_decision` ALWAYS wins.
async fn action_approve_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("plan `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .plan_update_status(id, PlanStatus::Approved)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "plan", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
        let outcome = evaluate_review_automation(automation_policy, &ctx, Some(input.decision));
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    // wave-21 / task 06 :: propose-only Sonnet pass for the explicit-
    // resolution path. Caller decision ALWAYS wins; proposal is
    // informational only.
    //
    // wave-22 / task 03 :: apply gate is INFORMATIONAL ONLY here. The
    // explicit `review_decision` already drove (or refused) the DB
    // transition above. We do NOT fail-fast on hash mismatch — that
    // would lie about state. The gate block surfaces the verdict for
    // audit symmetry.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            None,
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven approve path for `mission_plan`.
/// Fires when caller supplies `review_question_id` +
/// `review_automation_policy` (non-Manual) WITHOUT an explicit
/// `review_decision`. Auto-promotes to `Approved` only under `auto_safe`
/// + every safety rule passing. NEVER auto-rejects.
async fn plan_action_approve_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("plan `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
        "review_question_id": qid,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve {
        state
            .store
            .plan_update_status(id, PlanStatus::Approved)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["status"] = json!("approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        payload["status"] = json!("review_pending_decision");
        if matches!(outcome.status, AutomationStatus::AutoSafeBlocked) {
            payload["next_step"] = json!(
                "auto_safe blocked — supply explicit `review_decision` (approved|rejected|needs_changes) to flip the plan"
            );
        } else {
            payload["next_step"] = json!(
                "suggest mode is informational — supply explicit `review_decision` to flip the plan"
            );
        }
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve {
        maybe_emit_review_question_resolved(&mut payload, &state.bus, Some(&qid), "approved", None)
            .await;
    }

    // wave-21 / task 06 :: propose-only Sonnet pass on the policy-only
    // approve path.
    //
    // wave-22 / task 03 :: apply gate is INFORMATIONAL ONLY on this
    // path — the deterministic policy already drove the DB transition.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            None,
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}

pub(super) async fn action_mark(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;
    let target_raw = require_str(args, "status")?;
    let target = PlanStatus::from_str(target_raw).map_err(|e| {
        anyhow!(
            "`{}` is not a valid PlanStatus: {} (valid: draft|awaiting_approval|approved|executing|succeeded|failed|superseded)",
            target_raw,
            e
        )
    })?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. mark is
    // a general state-transition action; the gate only authorises
    // mark-to-approved (mirrors the wave-18 policy posture). For other
    // target statuses the gate falls through to a SKIP outcome.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge — same pattern as approve.
    // wave-18 / task 07 :: same MissingDecision-under-policy promotion.
    // mark is the most general state transition (caller picks the target
    // status) — so the policy can only auto-promote when the requested
    // target is `approved`. Other targets surface the suggestion only.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(
                    e,
                    crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision
                )
            {
                return Ok(ToolResult::structured_error(ToolError::new(
                    e.code(),
                    e.message(),
                )));
            }
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return plan_action_mark_with_policy_only(
                state,
                id,
                target,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_mark_with_resolution(
            state,
            id,
            target,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    state
        .store
        .plan_update_status(id, target)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "plan_id": id,
        "new_status": target.as_str(),
    });
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        target.as_str(),
        None,
    )
    .await;
    // wave-21 / task 06 :: propose-only Sonnet pass for the legacy mark
    // path. The requested target is surfaced to the prompt for context.
    //
    // wave-22 / task 03 :: apply gate is informational on this path —
    // the legacy mark already ran the requested transition above. For
    // a future wave that wants to gate mark-to-approved on the LLM
    // proposal, the gate block is the audit anchor.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            "legacy_quiet",
            "manual",
            false,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle =
            request_plan_auto_approve_proposal(state, mode, "mark", &id, 0, &summary, None).await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(&mut payload, &bundle, &apply_gate_input, &id, 0);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=mark`. Validates the
/// review envelope; on `approved` decision performs the requested
/// `plan_update_status` transition; on `rejected`/`needs_changes` keeps
/// the plan at its current status.
///
/// wave-18 / task 07 :: stamps the automation outcome on the response.
/// Caller-supplied `review_decision` always wins.
async fn action_mark_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    target: PlanStatus,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("plan `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
        "requested_status": target.as_str(),
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .plan_update_status(id, target)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["new_status"] = json!(target.as_str());
        }
        ResolutionOutcome::KeepArtifact => {
            payload["new_status"] = json!(plan.status.as_str());
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["new_status"] = json!(plan.status.as_str());
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "plan", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
        let outcome = evaluate_review_automation(automation_policy, &ctx, Some(input.decision));
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "mark",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: gate is informational — caller decision
        // already drove the transition above.
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven mark path. Auto-promotes ONLY
/// when the requested target status is `Approved` (the only safe
/// auto-resolution outcome for `mark`); other targets degrade to
/// suggest-only even when every safety rule passes.
async fn plan_action_mark_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    target: PlanStatus,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("plan `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
        "review_question_id": qid,
        "requested_status": target.as_str(),
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let mut ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    if !matches!(target, PlanStatus::Approved) {
        // `mark` to a non-Approved target is never auto-promoted by the
        // policy. We pin a loud blocker so the audit trail explains the
        // refusal even when every safety rule otherwise passes.
        ctx.additional_blockers.push(format!(
            "non_approved_target:mark target `{}` is never auto-promoted by review_automation_policy (only `approved` is)",
            target.as_str()
        ));
    }
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve && matches!(target, PlanStatus::Approved) {
        state
            .store
            .plan_update_status(id, PlanStatus::Approved)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["new_status"] = json!(PlanStatus::Approved.as_str());
        payload["status"] = json!("approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        payload["new_status"] = json!(plan.status.as_str());
        payload["status"] = json!("review_pending_decision");
        payload["next_step"] = json!(
            "supply explicit `review_decision` (approved|rejected|needs_changes) to finalise the mark"
        );
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve && matches!(target, PlanStatus::Approved) {
        maybe_emit_review_question_resolved(&mut payload, &state.bus, Some(&qid), "approved", None)
            .await;
    }

    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "mark",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: gate is informational on policy path —
        // deterministic policy already drove the transition.
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}

pub(super) async fn action_supersede(state: &AppState, args: &Value) -> Result<ToolResult> {
    let old_id = parse_id_arg(args, "old_plan_id")?;
    let new_id = parse_id_arg(args, "new_plan_id")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front. Supersede is destructive — the proposer ALWAYS
    // surfaces `destructive_blocked`.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front.
    // supersede is destructive (invariant I2) — the gate ALWAYS skips
    // with `SkippedDestructiveAction`. Strict shape errors still
    // surface as INVALID_PARAM here.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge. Supersede pivots two plan
    // ids; the review envelope is anchored to `old_plan_id` (the artifact
    // being closed out by the supersede). `Rejected` / `NeedsChanges` skip
    // the supersede entirely.
    //
    // wave-18 / task 07 :: supersede is destructive (the old plan goes
    // to Superseded). We never auto-promote it from a policy — the
    // policy-only branch surfaces the suggestion and refuses to mutate.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(
                    e,
                    crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision
                )
            {
                return Ok(ToolResult::structured_error(ToolError::new(
                    e.code(),
                    e.message(),
                )));
            }
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return plan_action_supersede_with_policy_only(
                state,
                old_id,
                new_id,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_supersede_with_resolution(
            state,
            old_id,
            new_id,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    state
        .store
        .plan_supersede(old_id, new_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "superseded",
        "old_plan_id": old_id,
        "new_plan_id": new_id,
    });
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        "superseded",
        None,
    )
    .await;
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            "legacy_quiet",
            "manual",
            false,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            0,
            &summary,
            None,
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(&mut payload, &bundle, &apply_gate_input, &old_id, 0);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=supersede`. Validates
/// the review envelope against the OLD plan (the artifact being closed),
/// then performs the supersede transition only when the decision is
/// `approved`.
///
/// wave-18 / task 07 :: stamps the automation outcome on the response.
/// Caller-supplied `review_decision` always wins.
async fn action_supersede_with_resolution(
    state: &AppState,
    old_id: uuid::Uuid,
    new_id: uuid::Uuid,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let plan = match state
        .store
        .plan_get(old_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("old plan `{}` not found for resolution", old_id),
            )))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &old_id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "old_plan_id": old_id,
        "new_plan_id": new_id,
        "version": current_version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .plan_supersede(old_id, new_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("superseded");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "plan", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
        let outcome = evaluate_review_automation(automation_policy, &ctx, Some(input.decision));
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    if let Some(mode) = proposer_mode {
        // supersede is destructive — proposer ALWAYS surfaces
        // `destructive_blocked` (invariant I2).
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &old_id,
            current_version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven supersede path. Supersede is
/// destructive (the old plan goes to `Superseded`); we never auto-promote
/// from a policy. Surfaces the suggestion only and refuses to mutate.
async fn plan_action_supersede_with_policy_only(
    state: &AppState,
    old_id: uuid::Uuid,
    new_id: uuid::Uuid,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let plan = match state
        .store
        .plan_get(old_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("old plan `{}` not found for resolution", old_id),
            )))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &old_id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "old_plan_id": old_id,
        "new_plan_id": new_id,
        "version": current_version,
        "review_question_id": qid,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let mut ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    ctx.additional_blockers.push(
        "destructive_action:supersede transitions are never auto-promoted by the automation policy"
            .to_string(),
    );
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    payload["status"] = json!("review_pending_decision");
    payload["next_step"] = json!(
        "supply explicit `review_decision` (approved|rejected|needs_changes) — supersede is destructive and never auto-promoted"
    );

    stamp_review_automation_payload(&mut payload, &outcome);

    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &old_id,
            current_version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}
