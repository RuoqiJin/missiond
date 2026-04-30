use super::*;

// ───────────────────────────────────────────────────────────────────────
// wave-16 :: subscriber-side resolution bridge
//
// Called by `bus::v2_subscribers::spawn_review_resolution_sub` after the
// pure planner classified the inbound `QuestionEvent::Resolved` event as a
// directive route. We re-validate the envelope (so a stale qid resolved
// against a since-updated directive bails loudly) and, ONLY for an
// `Approved` decision on a transition action, perform the same DB
// transition as the explicit caller-side bridge. We never re-publish a
// Resolved bus event — the inbound event we just consumed IS that signal.
// `Rejected` / `NeedsChanges` / `compile`-action ids never mutate state.
// ───────────────────────────────────────────────────────────────────────

/// Outcome of routing a `QuestionEvent::Resolved` event through the
/// directive-side bridge. Surfaced to the subscriber so it can record
/// observability without re-doing the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectiveSubscriberOutcome {
    /// Decision was `Approved` on `approve` action and the directive was
    /// transitioned to Approved (idempotent: no-op if already Approved).
    Approved,
    /// Decision was `Approved` on `archive` action and the directive was
    /// transitioned to Archived.
    Archived,
    /// Decision was `Rejected` or `NeedsChanges`; we left the directive
    /// at its current status.
    KeptArtifact { decision: ReviewDecision },
    /// Action was `compile` — there is no manager transition tied to
    /// the compile path itself (compile happens before review). We just
    /// log the resolution decision.
    CompileNoOp { decision: ReviewDecision },
    /// The qid envelope's `artifact_id` did not parse as a UUID.
    ArtifactIdNotUuid { artifact_id: String, error: String },
    /// Directive row was not found for the qid's artifact_id.
    NotFound { artifact_id: uuid::Uuid },
    /// Envelope failed re-validation (scope / version / action). Carries
    /// the error code so the subscriber can log a structured warning.
    EnvelopeRejected { code: &'static str, message: String },
    /// Underlying DB transition failed; the original `Resolved` event has
    /// already been consumed, so we surface the error as observability.
    DbError { detail: String },
}

/// Re-route a `QuestionEvent::Resolved` event whose envelope was parsed
/// as `scope=directive` through the same validators as the explicit
/// caller-side bridge. Performs the manager transition for an `Approved`
/// decision on an action in [`DIRECTIVE_REVIEW_ACTIONS`] except
/// `compile`. Pure side-effects: at most one DB write; no bus publish.
pub(crate) async fn handle_review_resolved_event(
    state: &AppState,
    parsed: &ParsedReviewQuestionId,
    decision: ReviewDecision,
) -> DirectiveSubscriberOutcome {
    let id = match uuid::Uuid::parse_str(&parsed.artifact_id) {
        Ok(u) => u,
        Err(e) => {
            return DirectiveSubscriberOutcome::ArtifactIdNotUuid {
                artifact_id: parsed.artifact_id.clone(),
                error: e.to_string(),
            }
        }
    };
    let chain = match state.store.directive_get_version_chain(id).await {
        Ok(c) => c,
        Err(e) => {
            return DirectiveSubscriberOutcome::DbError {
                detail: format!("directive_get_version_chain: {}", e),
            }
        }
    };
    let current_version = match chain.iter().last() {
        Some(d) => d.version,
        None => return DirectiveSubscriberOutcome::NotFound { artifact_id: id },
    };
    if let Err(e) = validate_review_resolution_envelope(
        parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return DirectiveSubscriberOutcome::EnvelopeRejected {
            code: e.code(),
            message: e.message(),
        };
    }
    if matches!(decision.outcome(), ResolutionOutcome::KeepArtifact)
        || matches!(decision.outcome(), ResolutionOutcome::RequestChanges)
    {
        return DirectiveSubscriberOutcome::KeptArtifact { decision };
    }
    // PerformTransition path. compile-action ids carry no manager
    // transition (compile happened pre-review); approve / archive do.
    match parsed.action.as_str() {
        "compile" => DirectiveSubscriberOutcome::CompileNoOp { decision },
        "approve" => match state.store.directive_approve(id, current_version).await {
            Ok(_) => DirectiveSubscriberOutcome::Approved,
            Err(e) => DirectiveSubscriberOutcome::DbError {
                detail: format!("directive_approve: {}", e),
            },
        },
        "archive" => match state
            .store
            .directive_update_status(id, current_version, DirectiveStatus::Archived)
            .await
        {
            Ok(_) => DirectiveSubscriberOutcome::Archived,
            Err(e) => DirectiveSubscriberOutcome::DbError {
                detail: format!("directive_update_status(archived): {}", e),
            },
        },
        // validate_review_resolution_envelope above already rejected
        // anything outside DIRECTIVE_REVIEW_ACTIONS, so this branch is
        // unreachable. Defensive fallback: treat as compile no-op.
        _ => DirectiveSubscriberOutcome::CompileNoOp { decision },
    }
}
