use super::*;

// ───────────────────────────────────────────────────────────────────────
// wave-16 :: subscriber-side resolution dispatcher
//
// The wave-16 `QuestionEvent::Resolved` listener (see
// `crates/missiond-daemon/src/bus/v2_subscribers.rs::spawn_review_resolution_sub`)
// consumes Resolved events and routes deterministic `review:*` ids back
// through the same explicit-resolution validators each manager handler
// already owns. This block is the PURE planner the subscriber uses so the
// dispatch logic is testable without spinning a bus / DB.
//
// Conservatism contract:
//   * Non-review ids → `IgnoreNonReviewId`. We never auto-act on
//     decision-engine answers for non-review questions.
//   * Malformed `review:*` ids → `IgnoreMalformedId`. Surface the parse
//     error so observability sees a loud signal; do NOT mutate.
//   * Resolution string outside the recognised vocabulary →
//     `IgnoreUnknownResolution`. Wider than the strict `ReviewDecision::parse`
//     vocabulary so wave-14 emitters / decision-engine tier output that
//     uses synonyms (`yes` / `accepted` / `fix` / `revise` / etc.) still
//     resolves cleanly.
//   * Recognised review id + recognised resolution → `Route` carrying the
//     parsed envelope and decision. The subscriber applies the per-scope
//     side-effect (no double-publish; the inbound Resolved is the
//     downstream signal).
// ───────────────────────────────────────────────────────────────────────

/// Dispatch outcome for a `(question_id, resolution)` pair pulled off the
/// `QuestionEvent::Resolved` topic. Pure / side-effect free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReviewResolvedDispatch {
    /// `question_id` does not start with the `review:` prefix — wave-16
    /// subscriber ignores it (other consumers may handle it).
    IgnoreNonReviewId,
    /// `question_id` started with `review:` but the envelope was malformed
    /// (missing segments / bad version / empty segment / unsupported
    /// scope). Subscriber logs a warning and acks; never mutates.
    IgnoreMalformedId(ReviewIdParseError),
    /// `question_id` was a recognised wave-14 id but the envelope's scope
    /// is not one of `directive | plan | workflow`. Defensive against
    /// third-party emitters that reuse the `review:` prefix.
    IgnoreUnsupportedScope { scope: String },
    /// Resolution string did not map to any known decision after the
    /// conservative vocabulary expansion. Subscriber logs a warning and
    /// acks; never mutates.
    IgnoreUnknownResolution { resolution: String },
    /// Recognised review id + recognised resolution → subscriber may
    /// route through the per-scope handler (`directive::resolve_from_event`
    /// / `plan::resolve_from_event` / `workflow::resolve_from_event`).
    Route {
        parsed: ParsedReviewQuestionId,
        decision: ReviewDecision,
    },
}

/// Conservative resolution-string mapper for the subscriber path. WIDER
/// vocabulary than `ReviewDecision::parse` (which is the strict explicit
/// API):
///
///   * `approved | approve | yes | accepted` → `Approved`
///   * `rejected | reject | no`              → `Rejected`
///   * `needs_changes | needs-changes | changes | revise | fix`
///                                            → `NeedsChanges`
///   * anything else                          → `None` (caller logs a
///     warning and acks the bus message; no mutation)
///
/// Case-insensitive + trimmed. Returns `None` rather than `Err` because
/// the subscriber never fails — it just ignores unknown vocabulary.
pub(crate) fn parse_subscriber_resolution_string(raw: &str) -> Option<ReviewDecision> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "approved" | "approve" | "yes" | "accepted" => Some(ReviewDecision::Approved),
        "rejected" | "reject" | "no" => Some(ReviewDecision::Rejected),
        "needs_changes" | "needs-changes" | "changes" | "revise" | "fix" => {
            Some(ReviewDecision::NeedsChanges)
        }
        _ => None,
    }
}

/// Pure planner: decide what the `QuestionEvent::Resolved` subscriber
/// should do with a `(question_id, resolution)` pair. Does NOT touch DB
/// or bus.
pub(crate) fn plan_review_resolved_dispatch(
    question_id: &str,
    resolution: &str,
) -> ReviewResolvedDispatch {
    if !question_id.trim_start().starts_with("review:") {
        return ReviewResolvedDispatch::IgnoreNonReviewId;
    }
    let parsed = match parse_review_question_id_struct(question_id) {
        Ok(p) => p,
        Err(e) => return ReviewResolvedDispatch::IgnoreMalformedId(e),
    };
    if !WAVE14_SUPPORTED_SCOPES.contains(&parsed.scope.as_str()) {
        return ReviewResolvedDispatch::IgnoreUnsupportedScope {
            scope: parsed.scope.clone(),
        };
    }
    let decision = match parse_subscriber_resolution_string(resolution) {
        Some(d) => d,
        None => {
            return ReviewResolvedDispatch::IgnoreUnknownResolution {
                resolution: resolution.to_string(),
            }
        }
    };
    ReviewResolvedDispatch::Route { parsed, decision }
}
