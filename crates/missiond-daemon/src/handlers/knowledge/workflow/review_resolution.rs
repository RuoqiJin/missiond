use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::handlers::knowledge::review_gate::{
    evaluate_review_automation, maybe_emit_review_question_resolved,
    parse_resolution_review_question_id, parse_review_automation_policy,
    parse_review_question_id_struct, parse_review_resolution_input, resolution_wire_string,
    review_automation_policy_was_explicit, stamp_needs_changes_next_step, stamp_resolution_payload,
    stamp_review_automation_payload, validate_review_resolution_envelope, AutomationStatus,
    ParsedReviewQuestionId, ResolutionOutcome, ReviewAutomationContext, ReviewAutomationPolicy,
    ReviewDecision, ReviewResolutionInput,
};
use crate::state::AppState;

// ───────────────────────────────────────────────────────────────────────
// wave-16 :: explicit review-resolution surface
//
// Closes the Wave-15 gap: directive / plan already accept `review_question_id
// + review_decision + review_actor + review_note` to flip an artifact from
// the auto-emitted `QuestionEvent::Created` (wave-14) into an explicit
// `QuestionEvent::Resolved` / `DecisionResolved`. Workflow auto-emits the
// same Created envelopes (scope = `workflow`, see `apply_compile_review_gates`
// calls in `action_distill_*` and `action_compile_deterministic`) but had
// no resolution surface — Wave-16 adds it here.
//
// Two forms share one entry point because the auto-emitter uses the same
// scope label (`workflow`) for both:
//
//   1. Persisted distill row — `artifact_id` parses as a UUID and the
//      `workflow_get_by_id` lookup returns Some. The `Workflow` row has no
//      version / status fields (unlike Directive / Plan), so the resolver
//      neither needs nor performs an "approve transition"; on `approved`
//      it stamps `status=review_approved` so the response is loud, on
//      `rejected` / `needs_changes` it stamps the matching review status
//      AND `next_step`. Bus emission is best-effort, mirroring directive /
//      plan.
//
//   2. compile_methodology compiled YAML — `artifact_id` is the `flow_id`
//      string (NOT a UUID; see `derive_flow_id` → `methodology-<stem>-v0`).
//      No DB row exists, so the resolver returns a STRUCTURED RECEIPT and
//      never fakes DB state. The receipt + Resolved bus event both carry
//      the deterministic question id so an external archiver / audit
//      pipeline can correlate.
//
// Action whitelist: only `compile`. The wave-14 auto-emitter always uses
// action=`compile` for workflow ids (see `apply_compile_review_gates(...)`
// → `auto_emit_review_question_after_artifact_write` default action). If
// callers ever opt into a custom id with a different action, the envelope
// validator will surface `REVIEW_ACTION_UNSUPPORTED` and force them to
// reconsider.
//
// Scope label: `workflow` for BOTH persisted and methodology paths.
// (Wave-16 task brief sketched a separate `methodology` scope; the actual
// wave-14 derivation in `review_gate.rs` uses `workflow` for both — we
// match the existing emitter to keep ids round-trippable. The methodology
// path is distinguished by the artifact_id NOT being a UUID.)
// ───────────────────────────────────────────────────────────────────────

/// Action whitelist for the workflow surface. The wave-14 auto-emitter
/// always uses `compile` (see `auto_emit_review_question_after_artifact_write`
/// default), so this is the only action a workflow review id can carry.
pub(super) const WORKFLOW_REVIEW_ACTIONS: &[&str] = &["compile"];

/// Workflow review version. The `Workflow` row has no `version` column;
/// the auto-emitter pins all workflow ids to `v1` (see `apply_compile_review_gates`
/// calls in `action_distill_*` / `action_compile_deterministic`). Resolution
/// must validate against the same constant so a re-emit / retry stays
/// deterministic.
pub(super) const WORKFLOW_REVIEW_VERSION: i32 = 1;

pub(super) async fn action_resolve_review(state: &AppState, args: &Value) -> Result<ToolResult> {
    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // Caller must supply both id + decision (with optional actor / note).
    // Missing decision when id is present is fail-fast — same contract as
    // directive / plan.
    //
    // wave-18 / task 07 :: when a non-Manual `review_automation_policy`
    // is supplied without an explicit `review_decision`, route through
    // the policy bridge instead of fail-fast.
    let resolution = match parse_review_resolution_input(args) {
        Ok(Some(r)) => r,
        Ok(None) => {
            // No qid at all → still fail-fast (the policy needs an id to
            // anchor the decision against).
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "resolve_review requires `review_question_id` (and `review_decision`)",
                )
                .with_suggestion(
                    "use the deterministic id wave-14 emitted on the workflow Created event",
                ),
            ));
        }
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
            return action_resolve_review_with_policy_only(state, qid, automation_policy).await;
        }
    };

    let parsed = match parse_review_question_id_struct(&resolution.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )));
        }
    };

    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "workflow",
        &parsed.artifact_id,
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    // Decide between persisted-row mode and methodology-receipt mode by
    // attempting a UUID parse on the envelope's artifact_id. Methodology
    // flow ids look like `methodology-<stem>-v0` and never parse as UUID.
    match uuid::Uuid::parse_str(&parsed.artifact_id) {
        Ok(workflow_id) => {
            action_resolve_review_persisted(
                state,
                workflow_id,
                resolution,
                automation_policy,
                automation_explicit,
            )
            .await
        }
        Err(_) => {
            action_resolve_review_methodology(
                state,
                parsed.artifact_id.clone(),
                resolution,
                automation_policy,
                automation_explicit,
            )
            .await
        }
    }
}

/// Wave-18 / task 07 :: build the workflow-side automation context. We
/// have NO `compiler_model` field on the Workflow row, so the
/// deterministic-mode rule defaults to `false` for persisted workflows
/// (distill historically ran the LLM-driven Sonnet path). The methodology
/// branch (compile_methodology) is fully deterministic — its
/// `compile_mode="deterministic"` writes a YAML from the source `.lisp`
/// without an LLM, so we flag those callers via `methodology_branch=true`.
///
/// The defaults here intentionally err on the side of CAUTION:
/// `auto_safe` for workflow approvals will not auto-promote unless the
/// caller explicitly passes `deterministic_workflow=true` — surfacing
/// the rule outcomes loudly in the response so an operator can see why.
fn build_workflow_automation_ctx(
    args: &Value,
    methodology_branch: bool,
) -> ReviewAutomationContext {
    let caller_deterministic = args
        .get("deterministic_workflow")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let deterministic_mode = methodology_branch || caller_deterministic;
    ReviewAutomationContext {
        deterministic_mode,
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

/// Wave-18 / task 07 :: policy-driven resolve_review path. Workflow rows
/// have no status column; the persisted branch never runs a DB
/// transition. The policy can still surface a suggestion under
/// `auto_safe` / `suggest`. NEVER mutates state.
async fn action_resolve_review_with_policy_only(
    state: &AppState,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )));
        }
    };
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "workflow",
        &parsed.artifact_id,
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let methodology_branch = uuid::Uuid::parse_str(&parsed.artifact_id).is_err();
    // Surface enough context for the operator to act.
    let mut payload = json!({
        "scope": "workflow",
        "mode": if methodology_branch { "methodology" } else { "persisted" },
        "artifact_id": parsed.artifact_id,
        "version": WORKFLOW_REVIEW_VERSION,
        "review_question_id": qid,
        "db_transition": false,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let ctx = build_workflow_automation_ctx(&args_v, methodology_branch);
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve {
        // Workflow has no DB transition — auto-promotion just stamps the
        // approval on the response and emits the Resolved bus event.
        payload["status"] = json!("review_approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        payload["status"] = json!("review_pending_decision");
        if matches!(outcome.status, AutomationStatus::AutoSafeBlocked) {
            payload["next_step"] = json!(
                "auto_safe blocked — supply explicit `review_decision` (approved|rejected|needs_changes) to flip the workflow"
            );
        } else {
            payload["next_step"] = json!(
                "suggest mode is informational — supply explicit `review_decision` to flip the workflow"
            );
        }
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve {
        maybe_emit_review_question_resolved(&mut payload, &state.bus, Some(&qid), "approved", None)
            .await;
    }

    Ok(ToolResult::json_pretty(&payload))
}

/// Persisted distill resolution. The workflow row exists; the `Workflow`
/// type has no version / status fields, so the resolver does not perform a
/// DB transition — it stamps the decision into the response and emits the
/// Resolved bus event. `approved` is loud (`status=review_approved`);
/// `rejected` / `needs_changes` keep the artifact non-approved with the
/// reason surfaced.
///
/// wave-18 / task 07 :: stamps the automation outcome on the response.
/// Caller-supplied `review_decision` always wins.
async fn action_resolve_review_persisted(
    state: &AppState,
    workflow_id: uuid::Uuid,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
) -> Result<ToolResult> {
    let row = state
        .store
        .workflow_get_by_id(workflow_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let row = match row {
        Some(w) => w,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("workflow `{}` not found for resolution", workflow_id),
            )));
        }
    };

    let mut payload = json!({
        "scope": "workflow",
        "mode": "persisted",
        "workflow_id": row.id,
        "workflow_name": row.name,
        "version": WORKFLOW_REVIEW_VERSION,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            // The Workflow row has no status column to flip — record the
            // approval loudly in the response so callers see the decision
            // landed (the bus Resolved event carries the same).
            payload["status"] = json!("review_approved");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "workflow", "distill");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_workflow_automation_ctx(&args_v, false);
        let outcome = evaluate_review_automation(automation_policy, &ctx, Some(input.decision));
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
    Ok(ToolResult::json_pretty(&payload))
}

/// Methodology compiled-YAML resolution. NO DB workflow row exists —
/// `compile_methodology` only writes a `.missiond/generated/flows/<flow_id>.yaml`
/// (and optionally mirrors the source under `.missiond/workflows/<topic>.lisp`).
/// The resolver returns a structured receipt so an external archiver /
/// audit pipeline can correlate the decision with the source artifact,
/// AND emits the Resolved bus event (best-effort). It NEVER fakes DB
/// state — there is nothing to mutate.
///
/// wave-18 / task 07 :: stamps the automation outcome on the response.
/// methodology branch is fully deterministic (`compile_methodology` runs
/// no LLM), so the deterministic-mode rule auto-passes for this branch.
async fn action_resolve_review_methodology(
    state: &AppState,
    flow_id: String,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
) -> Result<ToolResult> {
    let mut payload = json!({
        "scope": "workflow",
        "mode": "methodology",
        "flow_id": flow_id,
        "version": WORKFLOW_REVIEW_VERSION,
        "db_transition": false,
        "note": "compile_methodology has no workflow row; resolution returns a receipt and emits the Resolved bus event without DB mutation",
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            payload["status"] = json!("review_approved");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "workflow", "compile_methodology");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_workflow_automation_ctx(&args_v, true);
        let outcome = evaluate_review_automation(automation_policy, &ctx, Some(input.decision));
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
    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// wave-16 :: subscriber-side resolution bridge
//
// Called by `bus::v2_subscribers::spawn_review_resolution_sub` after the
// pure planner classified the inbound `QuestionEvent::Resolved` event as a
// workflow route. Workflow rows have no `status` / `version` columns, so
// even on Approved we never perform a DB transition — the resolution is
// receipt-only (mirrors `action_resolve_review_persisted`). The
// methodology path (artifact_id is the flow_id string, not a UUID) is
// also receipt-only. The subscriber records the outcome and never
// re-publishes a Resolved bus event.
// ───────────────────────────────────────────────────────────────────────

/// Outcome of routing a `QuestionEvent::Resolved` event through the
/// workflow-side bridge. Surfaced to the subscriber so it can record
/// observability without re-doing the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkflowSubscriberOutcome {
    /// `artifact_id` parsed as UUID and the workflow row was found. No
    /// DB transition is performed (Workflow has no status column); the
    /// decision is receipt-only.
    PersistedReceipt {
        workflow_id: uuid::Uuid,
        decision: ReviewDecision,
    },
    /// `artifact_id` did not parse as UUID — interpreted as a methodology
    /// flow_id (e.g. `methodology-deploy-v0`). Receipt-only; never fakes
    /// DB state because no DB row exists.
    MethodologyReceipt {
        flow_id: String,
        decision: ReviewDecision,
    },
    /// `artifact_id` parsed as UUID but no workflow row was found.
    NotFound { artifact_id: uuid::Uuid },
    /// Envelope failed re-validation (scope / version / action).
    EnvelopeRejected { code: &'static str, message: String },
    /// Underlying DB lookup failed.
    DbError { detail: String },
}

/// Re-route a `QuestionEvent::Resolved` event whose envelope was parsed
/// as `scope=workflow` through the same validators as the explicit
/// caller-side bridge. Workflow rows carry no status column, so even an
/// `Approved` decision never mutates DB state — the outcome is
/// receipt-only. Pure side-effects: at most one DB read; no bus publish.
pub(crate) async fn handle_review_resolved_event(
    state: &AppState,
    parsed: &ParsedReviewQuestionId,
    decision: ReviewDecision,
) -> WorkflowSubscriberOutcome {
    if let Err(e) = validate_review_resolution_envelope(
        parsed,
        "workflow",
        &parsed.artifact_id,
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    ) {
        return WorkflowSubscriberOutcome::EnvelopeRejected {
            code: e.code(),
            message: e.message(),
        };
    }
    match uuid::Uuid::parse_str(&parsed.artifact_id) {
        Ok(workflow_id) => match state.store.workflow_get_by_id(workflow_id).await {
            Ok(Some(_)) => WorkflowSubscriberOutcome::PersistedReceipt {
                workflow_id,
                decision,
            },
            Ok(None) => WorkflowSubscriberOutcome::NotFound {
                artifact_id: workflow_id,
            },
            Err(e) => WorkflowSubscriberOutcome::DbError {
                detail: format!("workflow_get_by_id: {}", e),
            },
        },
        Err(_) => WorkflowSubscriberOutcome::MethodologyReceipt {
            flow_id: parsed.artifact_id.clone(),
            decision,
        },
    }
}
