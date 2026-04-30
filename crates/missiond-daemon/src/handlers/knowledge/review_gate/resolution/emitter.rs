use super::*;

// ───────────────────────────────────────────────────────────────────────
// Decision-time review gate (Resolved / DecisionResolved)
//
// approve / archive / mark / supersede call this when the caller passes
// `review_question_id`. The handler always succeeds first (DB mutation)
// and then attempts the publish. We never block the DB outcome on a bus
// success.
// ───────────────────────────────────────────────────────────────────────

/// Parse a single `review_question_id` field for the resolution path.
/// Returns `None` when absent / blank — the resolution emit is opt-in.
pub(crate) fn parse_resolution_review_question_id(args: &Value) -> Option<String> {
    args.get("review_question_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Optional decision metadata for `QuestionEvent::DecisionResolved`. When
/// `tier` is provided, the helper publishes `DecisionResolved` instead of
/// `Resolved` so the upstream router can attribute the decision tier.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolutionDecisionMeta {
    pub(crate) tier: Option<String>,
    pub(crate) duration_ms: Option<u64>,
}

/// Pure constructor: build the `QuestionEvent` that the resolution helper
/// would emit for a given `(qid, resolution, decision_meta)` triple. Split
/// out so tests can assert event shape without touching a real bus.
pub(crate) fn build_resolution_event(
    qid: &str,
    resolution: &str,
    decision: Option<&ResolutionDecisionMeta>,
) -> QuestionEvent {
    match decision.and_then(|d| d.tier.as_deref()) {
        Some(tier) => QuestionEvent::DecisionResolved {
            question_id: qid.to_string(),
            tier: tier.to_string(),
            duration_ms: decision.and_then(|d| d.duration_ms).unwrap_or(0),
        },
        None => QuestionEvent::Resolved {
            question_id: qid.to_string(),
            resolution: resolution.to_string(),
        },
    }
}

/// Pure event-kind label for response payload. Mirrors the
/// `DomainEvent::kind` impl on `QuestionEvent` but is callable from a
/// borrowed reference without touching the trait.
pub(crate) fn event_kind_label(ev: &QuestionEvent) -> &'static str {
    match ev {
        QuestionEvent::DecisionResolved { .. } => "decision_resolved",
        QuestionEvent::Resolved { .. } => "resolved",
        QuestionEvent::Created { .. } => "created",
    }
}

/// Best-effort `QuestionEvent::Resolved` (or `DecisionResolved` when
/// `decision.tier.is_some()`) emit after a control action (approve /
/// archive / mark / supersede) committed.
///
/// Mutates `payload`:
///   * `review_question_resolved` (bool) — true on success
///   * `review_question_id`       (string) — echoed back so the caller can
///     correlate with the original Created event
///   * `review_question_warning`  (object) — only when publish errored
///
/// Resolution is OPT-IN at the caller. When `qid.is_none()`, this helper is
/// a no-op (no payload mutation) so legacy callers that never knew about
/// the gate stay byte-identical.
pub(crate) async fn maybe_emit_review_question_resolved(
    payload: &mut Value,
    bus: &BusServices,
    qid: Option<&str>,
    resolution: &str,
    decision: Option<&ResolutionDecisionMeta>,
) {
    let Some(qid) = qid else {
        return;
    };
    let qid = qid.to_string();

    let ev = build_resolution_event(&qid, resolution, decision);
    let kind = event_kind_label(&ev);

    match bus.publish_question(ev).await {
        Ok(_) => {
            payload["review_question_resolved"] = json!(true);
            payload["review_question_id"] = json!(qid);
            payload["review_question_kind"] = json!(kind);
        }
        Err(e) => {
            warn!(
                question_id = %qid,
                resolution = resolution,
                kind = kind,
                error = %e,
                "review-gate: QuestionEvent resolved publish failed; DB action already committed"
            );
            payload["review_question_resolved"] = json!(false);
            payload["review_question_id"] = json!(qid);
            payload["review_question_kind"] = json!(kind);
            payload["review_question_warning"] = json!({
                "code": "BUS_PUBLISH_FAILED",
                "reason": format!("{:#}", e),
                "question_id": qid,
                "resolution": resolution,
                "kind": kind,
            });
        }
    }
}
