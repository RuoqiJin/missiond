use super::*;

// ───────────────────────────────────────────────────────────────────────
// wave-16 :: subscriber-side resolution bridge
//
// Called by `bus::v2_subscribers::spawn_review_resolution_sub` after the
// pure planner classified the inbound `QuestionEvent::Resolved` event as a
// plan route. Re-validates the envelope (so a stale qid resolved against
// a since-updated plan bails loudly) and, ONLY for an `Approved` decision
// on a transition action, performs the same DB transition as the explicit
// caller-side bridge. We never re-publish a Resolved bus event — the
// inbound event we just consumed IS that signal. `Rejected` /
// `NeedsChanges` / `compile`-action ids never mutate state.
//
// Note: `supersede` is supported by the explicit caller-side bridge
// (callers must pass both `old_plan_id` + `new_plan_id`). The subscriber
// path has only the qid envelope (which carries the OLD plan id), so we
// classify supersede as `SupersedeNeedsExplicitCall` and let the
// subscriber log a structured warning. Plan supersede should always be
// driven by an explicit operator, not an inferred bus event.
// ───────────────────────────────────────────────────────────────────────

/// Outcome of routing a `QuestionEvent::Resolved` event through the
/// plan-side bridge. Surfaced to the subscriber so it can record
/// observability without re-doing the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanSubscriberOutcome {
    /// Decision was `Approved` on `approve` action and the plan was
    /// transitioned to `PlanStatus::Approved`.
    Approved,
    /// Decision was `Approved` on `mark` action — but the subscriber path
    /// has no target status (the `mark` qid envelope encodes the action
    /// label, not the target column value), so we DO NOT transition. The
    /// caller-side `mark` flow must be used for status flips.
    MarkNeedsExplicitCall,
    /// Decision was `Approved` on `supersede` action — but the subscriber
    /// only has the OLD plan id from the envelope; it cannot infer the
    /// NEW plan id. Supersede requires an explicit operator call.
    SupersedeNeedsExplicitCall,
    /// Decision was `Rejected` or `NeedsChanges`; left the plan at its
    /// current status.
    KeptArtifact { decision: ReviewDecision },
    /// Action was `compile` — no manager transition tied to compile path.
    CompileNoOp { decision: ReviewDecision },
    /// Envelope's `artifact_id` did not parse as a UUID.
    ArtifactIdNotUuid { artifact_id: String, error: String },
    /// Plan row was not found for the qid's artifact_id.
    NotFound { artifact_id: uuid::Uuid },
    /// Envelope failed re-validation (scope / version / action).
    EnvelopeRejected { code: &'static str, message: String },
    /// Underlying DB transition failed; the inbound `Resolved` event has
    /// already been consumed, so we surface the error as observability.
    DbError { detail: String },
}

/// Re-route a `QuestionEvent::Resolved` event whose envelope was parsed
/// as `scope=plan` through the same validators as the explicit
/// caller-side bridge. Performs `plan_update_status(Approved)` for an
/// `Approved` decision on `approve` action; classifies `mark` /
/// `supersede` as needing-explicit-call (those actions carry parameters
/// not present in the qid envelope). Pure side-effects: at most one DB
/// write; no bus publish.
pub(crate) async fn handle_review_resolved_event(
    state: &AppState,
    parsed: &ParsedReviewQuestionId,
    decision: ReviewDecision,
) -> PlanSubscriberOutcome {
    let id = match uuid::Uuid::parse_str(&parsed.artifact_id) {
        Ok(u) => u,
        Err(e) => {
            return PlanSubscriberOutcome::ArtifactIdNotUuid {
                artifact_id: parsed.artifact_id.clone(),
                error: e.to_string(),
            };
        }
    };
    let plan = match state.store.plan_get(id).await {
        Ok(Some(p)) => p,
        Ok(None) => return PlanSubscriberOutcome::NotFound { artifact_id: id },
        Err(e) => {
            return PlanSubscriberOutcome::DbError {
                detail: format!("plan_get: {}", e),
            };
        }
    };
    if let Err(e) = validate_review_resolution_envelope(
        parsed,
        "plan",
        &id.to_string(),
        plan.version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return PlanSubscriberOutcome::EnvelopeRejected {
            code: e.code(),
            message: e.message(),
        };
    }
    if matches!(decision.outcome(), ResolutionOutcome::KeepArtifact)
        || matches!(decision.outcome(), ResolutionOutcome::RequestChanges)
    {
        return PlanSubscriberOutcome::KeptArtifact { decision };
    }
    match parsed.action.as_str() {
        "compile" => PlanSubscriberOutcome::CompileNoOp { decision },
        "approve" => match state
            .store
            .plan_update_status(id, PlanStatus::Approved)
            .await
        {
            Ok(_) => PlanSubscriberOutcome::Approved,
            Err(e) => PlanSubscriberOutcome::DbError {
                detail: format!("plan_update_status(approved): {}", e),
            },
        },
        "mark" => PlanSubscriberOutcome::MarkNeedsExplicitCall,
        "supersede" => PlanSubscriberOutcome::SupersedeNeedsExplicitCall,
        // validate_review_resolution_envelope above already rejected
        // anything outside PLAN_REVIEW_ACTIONS.
        _ => PlanSubscriberOutcome::CompileNoOp { decision },
    }
}
