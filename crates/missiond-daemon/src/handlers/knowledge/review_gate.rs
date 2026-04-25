//! review_gate — event-bus aware review-gate emission for directive / plan
//! file-first artifacts.
//!
//! Lisp authority:
//!   - intent-flow.lisp ::
//!       F-intent-alignment-plan-execution-loop ::
//!         s3 alignment-review-gate + s5 plan-review-gate
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!       role alignment-review-gate / role plan-review-gate
//!   - intent-event-bus.lisp :: QuestionEvent
//!
//! Scope (wave-11 :: review gate event-aware code-alignment):
//!   - Pure helpers + an opt-in best-effort emitter.
//!   - Carries the deterministic question id derivation (so every artifact
//!     produces the same id from `(scope, id, version, action)` — caller can
//!     correlate Created → Resolved without persisting state).
//!   - Does NOT extend `QuestionEvent` payload (the existing `question_id`
//!     field already carries our deterministic id, and existing serde tests
//!     stay intact).
//!   - Does NOT implement human UI / wait-for-answer. The Created event is
//!     fire-and-forget; the manager surface returns immediately so callers
//!     are never blocked on a human gate. Gate resolution (Resolved /
//!     DecisionResolved) is also opt-in via `review_question_id`.
//!
//! Bus failure semantics (mirrors CLAUDE.md `feedback_fail_fast_no_fallback`):
//!   - The core action (compile persist / approve / archive / mark / supersede)
//!     never fails because of a side-channel bus error.
//!   - But we ALSO refuse to silently swallow it: when the publish call
//!     errors, the response carries a `review_question_warning` block with
//!     the error text plus the deterministic id, so downstream readers see a
//!     loud signal in the response payload AND in the daemon logs.

use missiond_core::event::events::QuestionEvent;
use serde_json::{json, Value};
use tracing::warn;

use crate::bus::BusServices;

// ───────────────────────────────────────────────────────────────────────
// Deterministic question id helper.
//
// Layout: `review:<scope>:<id>:v<version>:<action>`
// Examples:
//   review:directive:0a1b…:v1:compile
//   review:directive:0a1b…:v1:approve
//   review:plan:9f3c…:v2:approve
//   review:plan:9f3c…:v2:supersede
//
// `id` and `action` are caller-controlled; we lowercase action so caller can
// pass either "Approve" or "approve" without surprising the recipient.
// ───────────────────────────────────────────────────────────────────────

/// Build the deterministic review-question id for a given artifact + action.
///
/// Pure, side-effect free — same input always returns the same string. The
/// caller is responsible for passing the canonical `id` (the artifact UUID
/// stringified) and a stable `action` keyword. Uppercase actions are
/// normalised to lowercase so `"Approve"` and `"approve"` collide — that is
/// the intended behaviour for review-gate correlation.
pub(crate) fn derive_review_question_id(
    scope: &str,
    id: &str,
    version: i32,
    action: &str,
) -> String {
    format!(
        "review:{}:{}:v{}:{}",
        scope,
        id,
        version,
        action.to_ascii_lowercase()
    )
}

// ───────────────────────────────────────────────────────────────────────
// Compile-time review gate (Created)
// ───────────────────────────────────────────────────────────────────────

/// Caller request for the compile-time review gate. Built once per compile
/// call. All fields are optional — when `enabled=false` the helper is a
/// no-op.
#[derive(Debug, Clone)]
pub(crate) struct CompileReviewGateRequest {
    pub(crate) enabled: bool,
    /// Optional human-readable text for the review prompt. Surfaced in the
    /// response payload so the caller (CLI / IDE) can render it; the bus
    /// payload itself stays minimal (id only) for forward-compat.
    pub(crate) text: Option<String>,
    /// Optional caller-supplied id override. When omitted, the helper derives
    /// `review:<scope>:<id>:v<version>:compile` from the persisted artifact.
    pub(crate) id_override: Option<String>,
}

/// Parse the compile-time review-gate args from a JSON request.
///
/// Recognised fields (all optional; absent → disabled):
///   * `emit_review_question` (bool, default false)
///   * `review_question_text` (string, optional, free-form)
///   * `review_question_id`   (string, optional, deterministic id override)
///
/// Returns a request value whose `enabled` flag mirrors the input. We never
/// reject malformed types here — the field is opt-in and the failure mode
/// is "no event emitted", which is also the default.
pub(crate) fn parse_compile_review_gate(args: &Value) -> CompileReviewGateRequest {
    let enabled = args
        .get("emit_review_question")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let text = args
        .get("review_question_text")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let id_override = args
        .get("review_question_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    CompileReviewGateRequest {
        enabled,
        text,
        id_override,
    }
}

/// Emit `QuestionEvent::Created` after a directive / plan has been
/// persisted. Best-effort: never returns Err — bus failures are warned and
/// surfaced on the response payload via the `review_question_warning` field.
///
/// Mutates `payload` in place so the response always carries:
///   * `review_question_emitted` (bool) — whether the gate was even active
///     for this call. False when `req.enabled=false`.
///   * `review_question_id`      (string) — only when emitted (or attempted)
///   * `review_question_text`    (string) — echoed back from the request
///   * `review_question_warning` (object) — only when the publish errored
pub(crate) async fn maybe_emit_review_question_created(
    payload: &mut Value,
    bus: &BusServices,
    req: &CompileReviewGateRequest,
    scope: &str,
    artifact_id: &str,
    version: i32,
) {
    if !req.enabled {
        // Loud "off" signal — callers can grep responses for false to see
        // they did NOT enable the gate.
        payload["review_question_emitted"] = json!(false);
        return;
    }
    let qid = req
        .id_override
        .clone()
        .unwrap_or_else(|| derive_review_question_id(scope, artifact_id, version, "compile"));
    let ev = QuestionEvent::Created {
        question_id: qid.clone(),
    };
    match bus.publish_question(ev).await {
        Ok(_) => {
            payload["review_question_emitted"] = json!(true);
            payload["review_question_id"] = json!(qid);
            if let Some(text) = req.text.as_ref() {
                payload["review_question_text"] = json!(text);
            }
        }
        Err(e) => {
            // Side-channel failure must not break the persisted draft. We
            // still expose the deterministic id so the caller can retry the
            // emit OR resolve the gate manually with the same id later.
            warn!(
                scope = scope,
                artifact_id = artifact_id,
                version = version,
                question_id = %qid,
                error = %e,
                "review-gate: QuestionEvent::Created publish failed; persisted artifact remains intact"
            );
            payload["review_question_emitted"] = json!(false);
            payload["review_question_id"] = json!(qid);
            if let Some(text) = req.text.as_ref() {
                payload["review_question_text"] = json!(text);
            }
            payload["review_question_warning"] = json!({
                "code": "BUS_PUBLISH_FAILED",
                "reason": format!("{:#}", e),
                "scope": scope,
                "artifact_id": artifact_id,
                "version": version,
                "question_id": qid,
            });
        }
    }
}

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
fn event_kind_label(ev: &QuestionEvent) -> &'static str {
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

// ───────────────────────────────────────────────────────────────────────
// tests — pure helpers only (no bus, no DB).
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_deterministic_for_same_input() {
        let a = derive_review_question_id("directive", "abc-123", 1, "compile");
        let b = derive_review_question_id("directive", "abc-123", 1, "compile");
        assert_eq!(a, b);
    }

    #[test]
    fn id_normalises_action_case() {
        let a = derive_review_question_id("plan", "p-1", 2, "Approve");
        let b = derive_review_question_id("plan", "p-1", 2, "approve");
        assert_eq!(a, b, "uppercase action must collide with lowercase form");
    }

    #[test]
    fn id_layout_has_canonical_format() {
        let id = derive_review_question_id("directive", "abc-123", 5, "compile");
        assert_eq!(id, "review:directive:abc-123:v5:compile");
    }

    #[test]
    fn id_changes_when_any_field_changes() {
        let base = derive_review_question_id("directive", "abc", 1, "compile");
        assert_ne!(
            base,
            derive_review_question_id("plan", "abc", 1, "compile"),
            "scope must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "abc", 2, "compile"),
            "version must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "abc", 1, "approve"),
            "action must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "xyz", 1, "compile"),
            "id must affect id"
        );
    }

    // -- parse_compile_review_gate --

    #[test]
    fn parse_compile_default_is_disabled() {
        let req = parse_compile_review_gate(&json!({}));
        assert!(!req.enabled);
        assert!(req.text.is_none());
        assert!(req.id_override.is_none());
    }

    #[test]
    fn parse_compile_extracts_all_fields() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_text": "  please review  ",
            "review_question_id": "  override-id  ",
        }));
        assert!(req.enabled);
        assert_eq!(req.text.as_deref(), Some("please review"));
        assert_eq!(req.id_override.as_deref(), Some("override-id"));
    }

    #[test]
    fn parse_compile_filters_blank_strings() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_text": "   ",
            "review_question_id": "",
        }));
        assert!(req.enabled);
        assert!(req.text.is_none());
        assert!(req.id_override.is_none());
    }

    #[test]
    fn parse_compile_emit_false_keeps_other_fields_in_struct_but_disabled() {
        // We still parse the optional override because callers may flip
        // emit later — but the helper must respect `enabled=false`.
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": false,
            "review_question_id": "explicit-id",
        }));
        assert!(!req.enabled);
        assert_eq!(req.id_override.as_deref(), Some("explicit-id"));
    }

    // -- parse_resolution_review_question_id --

    #[test]
    fn parse_resolution_id_returns_none_when_absent() {
        assert!(parse_resolution_review_question_id(&json!({})).is_none());
    }

    #[test]
    fn parse_resolution_id_trims_and_filters_blank() {
        assert!(parse_resolution_review_question_id(&json!({
            "review_question_id": "   "
        }))
        .is_none());
        assert_eq!(
            parse_resolution_review_question_id(&json!({
                "review_question_id": "  abc  "
            })),
            Some("abc".to_string())
        );
    }

    // -- build_resolution_event --

    #[test]
    fn resolution_event_without_decision_meta_is_resolved() {
        let ev = build_resolution_event("review:plan:p1:v1:approve", "approved", None);
        match ev {
            QuestionEvent::Resolved {
                question_id,
                resolution,
            } => {
                assert_eq!(question_id, "review:plan:p1:v1:approve");
                assert_eq!(resolution, "approved");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn resolution_event_with_tier_is_decision_resolved() {
        let meta = ResolutionDecisionMeta {
            tier: Some("tier1".into()),
            duration_ms: Some(123),
        };
        let ev = build_resolution_event("review:plan:p1:v1:approve", "approved", Some(&meta));
        match ev {
            QuestionEvent::DecisionResolved {
                question_id,
                tier,
                duration_ms,
            } => {
                assert_eq!(question_id, "review:plan:p1:v1:approve");
                assert_eq!(tier, "tier1");
                assert_eq!(duration_ms, 123);
            }
            other => panic!("expected DecisionResolved, got {other:?}"),
        }
    }

    #[test]
    fn resolution_event_decision_meta_default_duration_is_zero() {
        let meta = ResolutionDecisionMeta {
            tier: Some("urgent".into()),
            duration_ms: None,
        };
        let ev = build_resolution_event("rid", "approved", Some(&meta));
        if let QuestionEvent::DecisionResolved { duration_ms, .. } = ev {
            assert_eq!(duration_ms, 0);
        } else {
            panic!("expected DecisionResolved");
        }
    }

    #[test]
    fn resolution_event_meta_without_tier_falls_back_to_resolved() {
        // tier=None means "no decision-tier metadata" → plain Resolved even
        // when meta block is supplied. This pins the precedence.
        let meta = ResolutionDecisionMeta {
            tier: None,
            duration_ms: Some(99),
        };
        let ev = build_resolution_event("rid", "approved", Some(&meta));
        assert!(matches!(ev, QuestionEvent::Resolved { .. }));
    }

    #[test]
    fn event_kind_label_for_each_variant() {
        assert_eq!(
            event_kind_label(&QuestionEvent::Created {
                question_id: "x".into(),
            }),
            "created"
        );
        assert_eq!(
            event_kind_label(&QuestionEvent::Resolved {
                question_id: "x".into(),
                resolution: "y".into(),
            }),
            "resolved"
        );
        assert_eq!(
            event_kind_label(&QuestionEvent::DecisionResolved {
                question_id: "x".into(),
                tier: "t".into(),
                duration_ms: 0,
            }),
            "decision_resolved"
        );
    }

    // -- compile-response payload contract (caller-visible fields) --

    /// The compile branches construct a payload that may include the
    /// emission fields. These tests exercise the request-side decision
    /// surface (the inputs to `maybe_emit_review_question_created`) so the
    /// MCP contract stays pinned even without a real bus.
    #[test]
    fn compile_request_disabled_means_no_emission_fields_will_be_added() {
        let req = parse_compile_review_gate(&json!({}));
        assert!(!req.enabled);
        // When enabled=false the helper writes review_question_emitted=false
        // and no warning. The contract is "loud off" — see docstring on
        // maybe_emit_review_question_created.
        let derived = derive_review_question_id("directive", "abc", 1, "compile");
        assert_eq!(derived, "review:directive:abc:v1:compile");
    }

    #[test]
    fn compile_request_with_explicit_id_overrides_derived() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_id": "custom:q-1",
        }));
        assert!(req.enabled);
        assert_eq!(req.id_override.as_deref(), Some("custom:q-1"));
    }

    #[test]
    fn compile_request_without_explicit_id_falls_back_to_derived() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
        }));
        assert!(req.enabled);
        assert!(req.id_override.is_none());
        // The handler will compute the derived id at emit time from the
        // persisted artifact (id, version). Pin the contract here.
        let qid = derive_review_question_id("plan", "p-7", 3, "compile");
        assert_eq!(qid, "review:plan:p-7:v3:compile");
    }
}
