use super::*;

/// What the manager surface should DO after the resolution input passes
/// validation. Pure projection of `decision`:
///
///   * `Approved`     → `PerformTransition` — caller runs its existing
///                       state transition (e.g. `directive_approve`),
///                       then emits `Resolved/approved`.
///   * `Rejected`     → `KeepArtifact` — caller skips the transition,
///                       leaves the artifact non-approved, emits
///                       `Resolved/rejected` with the actor / note as the
///                       reason.
///   * `NeedsChanges` → `RequestChanges` — caller skips the transition,
///                       leaves the artifact in the review/draft path,
///                       emits `Resolved/needs_changes`, surfaces a
///                       `next_step` recommendation in the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionOutcome {
    PerformTransition,
    KeepArtifact,
    RequestChanges,
}

impl ReviewDecision {
    pub(crate) fn outcome(self) -> ResolutionOutcome {
        match self {
            ReviewDecision::Approved => ResolutionOutcome::PerformTransition,
            ReviewDecision::Rejected => ResolutionOutcome::KeepArtifact,
            ReviewDecision::NeedsChanges => ResolutionOutcome::RequestChanges,
        }
    }
}

/// Stamp the resolution decision + actor + note onto the response
/// payload. Always called after the manager surface decides what to do
/// (whether or not the DB transition fired). Pure / no bus calls.
///
/// Mutates `payload` with:
///   * `review_question_id`        (string) — echoed back so callers can
///     correlate with the original Created event.
///   * `review_decision`           (string) — `approved|rejected|needs_changes`.
///   * `review_decision_outcome`   (string) — `perform_transition|keep_artifact|request_changes`.
///   * `review_actor`              (string) — when supplied.
///   * `review_note`               (string) — when supplied.
pub(crate) fn stamp_resolution_payload(payload: &mut Value, input: &ReviewResolutionInput) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert("review_question_id".to_string(), json!(input.question_id));
    map.insert(
        "review_decision".to_string(),
        json!(input.decision.as_str()),
    );
    map.insert(
        "review_decision_outcome".to_string(),
        json!(match input.decision.outcome() {
            ResolutionOutcome::PerformTransition => "perform_transition",
            ResolutionOutcome::KeepArtifact => "keep_artifact",
            ResolutionOutcome::RequestChanges => "request_changes",
        }),
    );
    if let Some(actor) = input.actor.as_ref() {
        map.insert("review_actor".to_string(), json!(actor));
    }
    if let Some(note) = input.note.as_ref() {
        map.insert("review_note".to_string(), json!(note));
    }
}

/// Stamp the response payload for a `RequestChanges` (needs_changes)
/// resolution: surface a default `next_step` so the caller knows what to
/// do next.
pub(crate) fn stamp_needs_changes_next_step(payload: &mut Value, scope: &str, action: &str) {
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            "next_step".to_string(),
            json!(format!(
                "rework the {scope} draft per `review_note`, then re-run `mission_{scope}(action={action})` against the new version"
            )),
        );
    }
}

/// Build the structured event resolution string for a given decision.
/// Centralises the wire vocabulary so directive / plan / workflow speak
/// the same thing on the bus.
pub(crate) fn resolution_wire_string(decision: ReviewDecision) -> &'static str {
    decision.as_str()
}
