//! review_gate — event-bus aware review-gate emission for directive / plan /
//! workflow file-first artifacts.
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
//! Scope (wave-14 :: review gate auto-create v1):
//!   - Adds [`ReviewGatePolicy`] (`manual` / `emit_question` / `off`) and
//!     [`parse_review_gate_policy`] so directive / plan / workflow handlers
//!     can opt callers into automatic `QuestionEvent::Created` emission after
//!     a successful file-first artifact write — without changing the legacy
//!     opt-in `emit_review_question` boolean (which keeps working under the
//!     `manual` policy).
//!   - Adds [`auto_emit_review_question_after_artifact_write`], the
//!     post-write hook called from compile / distill paths AFTER
//!     `attempt_artifact_write` has spliced its `file_written` outcome. The
//!     hook only fires when policy=`emit_question` AND the splice declared
//!     `file_written=true`; otherwise it stamps `review_question_emitted=
//!     false` and surfaces the policy + reason so callers can observe what
//!     happened.
//!   - Deterministic id derivation is extended via
//!     [`derive_review_question_id_for_artifact`] which folds the artifact
//!     kind label, db id, version, and topic-or-file-path-hash into the same
//!     `review:<scope>:<id>:v<version>:<action>:<topic-hash>` envelope — same
//!     input always returns the same id, so retries / resolutions correlate
//!     even across daemon restarts.
//!   - Does NOT auto-approve, does NOT wait, does NOT mutate the persisted
//!     artifact. The hook is fire-and-forget on the bus side, and the file
//!     write success comes from the splice — we never overwrite the splice.
//!
//! Bus failure semantics (mirrors CLAUDE.md `feedback_fail_fast_no_fallback`):
//!   - The core action (compile persist / approve / archive / mark / supersede)
//!     never fails because of a side-channel bus error.
//!   - But we ALSO refuse to silently swallow it: when the publish call
//!     errors, the response carries a `review_question_warning` block with
//!     the error text plus the deterministic id, so downstream readers see a
//!     loud signal in the response payload AND in the daemon logs.

#[cfg(test)]
use missiond_core::event::events::QuestionEvent;
#[cfg(test)]
use serde_json::{json, Value};

mod created;
mod resolution;
mod auto_answer;
mod llm_approval;

#[allow(unused_imports)]
pub(crate) use auto_answer::{
    auto_answer_policy_was_explicit, evaluate_auto_answer_policy,
    is_destructive_review_action, parse_auto_answer_policy, stamp_auto_answer_payload,
    AutoAnswerOutcome, AutoAnswerPolicy, AutoAnswerStatus,
};
#[cfg(test)]
use auto_answer::DESTRUCTIVE_REVIEW_ACTIONS;

#[allow(unused_imports)]
pub(crate) use resolution::{
    build_resolution_event, evaluate_review_automation, maybe_emit_review_question_resolved,
    parse_plan_node_resume_input, parse_resolution_review_question_id,
    parse_review_automation_policy, parse_review_question_id_struct,
    parse_review_resolution_input, parse_subscriber_resolution_string,
    plan_review_resolved_dispatch, review_automation_policy_was_explicit,
    resolution_wire_string, stamp_needs_changes_next_step, stamp_resolution_payload,
    stamp_review_automation_payload, validate_review_resolution_envelope, AutomationStatus,
    ParsedReviewQuestionId, PlanNodeResumeInput, ResolutionDecisionMeta, ResolutionInputError,
    ResolutionOutcome, ResolutionValidationError, ReviewAutomationContext,
    ReviewAutomationOutcome, ReviewAutomationPolicy, ReviewDecision, ReviewIdParseError,
    ReviewResolutionInput, ReviewResolvedDispatch,
};
#[cfg(test)]
use resolution::event_kind_label;

#[allow(unused_imports)]
pub(crate) use created::{
    apply_compile_review_gates, auto_emit_review_question_after_artifact_write,
    derive_plan_node_review_question_id, derive_plan_node_topic_hash, derive_review_question_id,
    derive_review_question_id_for_artifact, is_plan_node_review_action,
    maybe_emit_review_question_created, parse_compile_review_gate, parse_review_gate_policy,
    review_gate_policy_was_explicit, AutoEmitDecision, CompileReviewGateRequest,
    ReviewGatePolicy, PLAN_NODE_REVIEW_DEFAULT_ACTION,
};
#[cfg(test)]
use created::{payload_says_file_written, stamp_policy, topic_hash_short};

#[allow(unused_imports)]
pub(crate) use llm_approval::{
    build_llm_auto_approve_proposal_system_prompt,
    build_llm_auto_approve_proposal_user_prompt, compute_proposal_hash,
    enforce_apply_gate_preflight, enforce_proposal_invariants,
    evaluate_llm_approve_apply_gate, llm_auto_approve_proposal_mode_was_explicit,
    parse_llm_approve_apply_gate_input, parse_llm_auto_approve_proposal,
    parse_llm_auto_approve_proposal_mode, stamp_llm_approve_apply_gate_payload,
    stamp_llm_auto_approve_proposal_payload, stamp_proposal_hash_payload,
    LlmApproveApplyGateInput, LlmApproveApplyGateOutcome, LlmApproveApplyStatus,
    LlmAutoApproveProposal, LlmAutoApproveProposalBundle, LlmAutoApproveProposalConfidence,
    LlmAutoApproveProposalMode, LlmAutoApproveProposalStatus, ProposalHashStatus,
    APPLY_GATE_INVALID_PARAM, APPLY_GATE_MISSING_PROPOSAL_HASH,
    APPLY_GATE_PROPOSAL_HASH_MISMATCH,
};
#[cfg(test)]
use llm_approval::{proposal_json_kind, strip_proposal_code_fence};

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

    // ── wave-14 :: review_gate_policy parser ─────────────────────────────

    #[test]
    fn parse_policy_default_is_manual() {
        assert_eq!(
            parse_review_gate_policy(&json!({})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn parse_policy_recognises_emit_question() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "emit_question"})),
            ReviewGatePolicy::EmitQuestion
        );
    }

    #[test]
    fn parse_policy_recognises_off() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "off"})),
            ReviewGatePolicy::Off
        );
    }

    #[test]
    fn parse_policy_is_case_insensitive_and_trims() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "  EMIT_QUESTION  "})),
            ReviewGatePolicy::EmitQuestion
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "Off"})),
            ReviewGatePolicy::Off
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "MANUAL"})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn parse_policy_unknown_collapses_to_manual() {
        // Unknown values are silently mapped to the default rather than
        // rejected — the response always echoes the resolved policy so a
        // typo is observable downstream.
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "always"})),
            ReviewGatePolicy::Manual
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": ""})),
            ReviewGatePolicy::Manual
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "   "})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn policy_label_round_trips() {
        assert_eq!(ReviewGatePolicy::Manual.as_str(), "manual");
        assert_eq!(ReviewGatePolicy::EmitQuestion.as_str(), "emit_question");
        assert_eq!(ReviewGatePolicy::Off.as_str(), "off");
    }

    // ── wave-14 :: deterministic id with topic / file-path hash ─────────

    #[test]
    fn artifact_id_appends_topic_hash_suffix() {
        let id = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("wave14-topic"),
        );
        assert!(
            id.starts_with("review:directive:abc:v1:compile:"),
            "expected legacy prefix, got: {id}"
        );
        // Suffix must be the truncated hash, NOT the raw topic — keeps the
        // id bounded and obfuscates topic length.
        let suffix = id.rsplit(':').next().unwrap();
        assert_eq!(suffix.len(), 16, "suffix must be 16 hex chars");
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!id.contains("wave14-topic"));
    }

    #[test]
    fn artifact_id_without_topic_falls_back_to_legacy_layout() {
        // Empty / blank `topic_or_path` collapses to the wave-11 layout so
        // existing callers that don't have a path yet stay byte-identical.
        let id = derive_review_question_id_for_artifact("plan", "p1", 2, "approve", None);
        assert_eq!(id, "review:plan:p1:v2:approve");
        let id2 =
            derive_review_question_id_for_artifact("plan", "p1", 2, "approve", Some("   "));
        assert_eq!(id2, "review:plan:p1:v2:approve");
    }

    #[test]
    fn artifact_id_is_deterministic_for_same_topic() {
        let a = derive_review_question_id_for_artifact(
            "workflow",
            "wf1",
            3,
            "compile",
            Some("/abs/path/.missiond/workflows/foo.lisp"),
        );
        let b = derive_review_question_id_for_artifact(
            "workflow",
            "wf1",
            3,
            "compile",
            Some("/abs/path/.missiond/workflows/foo.lisp"),
        );
        assert_eq!(a, b);
    }

    // ── wave-16 / task 04 — plan-node review-gate id helper ────────────

    #[test]
    fn plan_node_review_id_uses_plan_scope_and_topic_hash() {
        let id = derive_plan_node_review_question_id(
            "00000000-0000-0000-0000-000000000abc",
            3,
            "n1",
            None,
        );
        // scope=plan → wave-14 supported scope; topic-hash suffix folds in node_id.
        assert!(
            id.starts_with(
                "review:plan:00000000-0000-0000-0000-000000000abc:v3:plan-node:"
            ),
            "unexpected layout: {id}"
        );
        let suffix = id.rsplit(':').next().unwrap();
        assert_eq!(suffix.len(), 16, "16-hex topic hash expected");
    }

    #[test]
    fn plan_node_review_id_action_override_changes_id() {
        let default = derive_plan_node_review_question_id("p1", 1, "n1", None);
        let override_action =
            derive_plan_node_review_question_id("p1", 1, "n1", Some("human-checkpoint"));
        assert_ne!(default, override_action);
        assert!(override_action.contains(":human-checkpoint:"));
    }

    #[test]
    fn plan_node_review_id_blank_action_falls_back_to_default() {
        let blank = derive_plan_node_review_question_id("p1", 1, "n1", Some("   "));
        let default = derive_plan_node_review_question_id("p1", 1, "n1", None);
        assert_eq!(blank, default);
    }

    #[test]
    fn plan_node_review_id_distinct_per_node_under_same_plan() {
        let a = derive_plan_node_review_question_id("p1", 1, "node-a", None);
        let b = derive_plan_node_review_question_id("p1", 1, "node-b", None);
        assert_ne!(a, b, "different nodes must produce distinct ids");
    }

    #[test]
    fn plan_node_review_id_routes_under_plan_scope_via_subscriber() {
        // Forward-compat with wave16-02: the deterministic id must dispatch
        // under the existing `Route { scope=plan, ... }` outcome so the
        // QuestionEvent::Resolved listener can reach the per-scope handler
        // when auto-resume lands.
        let id = derive_plan_node_review_question_id(
            "00000000-0000-0000-0000-000000000abc",
            1,
            "n1",
            Some("plan-node"),
        );
        let dispatch = plan_review_resolved_dispatch(&id, "approved");
        match dispatch {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "plan");
                assert_eq!(parsed.action, "plan-node");
                assert!(parsed.topic_hash.is_some());
                assert_eq!(decision, ReviewDecision::Approved);
            }
            other => panic!("expected Route under plan scope, got {:?}", other),
        }
    }

    #[test]
    fn artifact_id_changes_when_topic_changes() {
        let a = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("topic-a"),
        );
        let b = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("topic-b"),
        );
        assert_ne!(a, b, "topic must affect the trailing hash");
    }

    #[test]
    fn topic_hash_short_is_16_hex_chars() {
        let h = topic_hash_short("anything");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn topic_hash_short_is_stable() {
        // Pin the exact prefix for "wave14-topic" so an accidental change to
        // the hashing scheme breaks loud (id correlation across daemon
        // restarts depends on stability).
        assert_eq!(topic_hash_short("wave14-topic").len(), 16);
        let a = topic_hash_short("wave14-topic");
        let b = topic_hash_short("wave14-topic");
        assert_eq!(a, b);
    }

    // ── wave-14 :: payload introspection helper ─────────────────────────

    #[test]
    fn payload_says_file_written_true_when_flag_present() {
        let p = json!({"file_written": true});
        assert!(payload_says_file_written(&p));
    }

    #[test]
    fn payload_says_file_written_false_when_flag_missing() {
        let p = json!({"status": "compiled"});
        assert!(!payload_says_file_written(&p));
    }

    #[test]
    fn payload_says_file_written_false_when_flag_false() {
        let p = json!({"file_written": false});
        assert!(!payload_says_file_written(&p));
    }

    #[test]
    fn stamp_policy_inserts_resolved_label() {
        let mut p = json!({"status": "compiled"});
        stamp_policy(&mut p, ReviewGatePolicy::EmitQuestion);
        assert_eq!(p["review_gate_policy"], "emit_question");
    }

    #[test]
    fn stamp_policy_overwrites_prior_value() {
        // Always overwrite — we treat `review_gate_policy` as authoritative
        // for the resolved policy on this call.
        let mut p = json!({
            "status": "compiled",
            "review_gate_policy": "off",
        });
        stamp_policy(&mut p, ReviewGatePolicy::Manual);
        assert_eq!(p["review_gate_policy"], "manual");
    }

    // ── wave-14 :: auto-emit decision matrix (no bus) ───────────────────
    //
    // We can't drive the actual `auto_emit_review_question_after_artifact_write`
    // helper here without a `BusServices`, but the manual / off / file-not-
    // written branches return BEFORE the publish call. Replay the same
    // payload mutations in pure helpers so the contract stays pinned.

    #[test]
    fn auto_emit_manual_branch_is_a_noop_aside_from_policy_stamp() {
        // Replay manual-branch behaviour: stamp policy + return early.
        let mut p = json!({"status": "compiled", "file_written": true});
        stamp_policy(&mut p, ReviewGatePolicy::Manual);
        assert_eq!(p["review_gate_policy"], "manual");
        // No `review_question_emitted` mutation on manual — we leave the
        // legacy explicit-emit path in control of that field.
        assert!(p.get("review_question_emitted").is_none());
    }

    #[test]
    fn auto_emit_off_branch_stamps_emitted_false() {
        let mut p = json!({"status": "compiled", "file_written": true});
        stamp_policy(&mut p, ReviewGatePolicy::Off);
        // Replay the off-branch mutation: stamp emitted=false if absent.
        if let Some(map) = p.as_object_mut() {
            map.entry("review_question_emitted".to_string())
                .or_insert(json!(false));
        }
        assert_eq!(p["review_question_emitted"], false);
        assert_eq!(p["review_gate_policy"], "off");
    }

    #[test]
    fn auto_emit_file_not_written_records_warning_without_publishing() {
        let mut p = json!({"status": "partial", "file_written": false});
        stamp_policy(&mut p, ReviewGatePolicy::EmitQuestion);
        // Replay the suppress-because-no-file branch.
        if let Some(map) = p.as_object_mut() {
            map.insert("review_question_emitted".to_string(), json!(false));
            map.entry("review_question_warning".to_string()).or_insert(json!({
                "code": "FILE_WRITE_NOT_SUCCESSFUL",
                "reason": "review_gate_policy=emit_question requires file_written=true; auto-emit suppressed",
                "scope": "directive",
                "artifact_id": "abc",
                "version": 1,
            }));
        }
        assert_eq!(p["review_question_emitted"], false);
        assert_eq!(p["review_gate_policy"], "emit_question");
        assert_eq!(p["review_question_warning"]["code"], "FILE_WRITE_NOT_SUCCESSFUL");
    }

    #[test]
    fn auto_emit_explicit_id_override_wins_over_derived() {
        // Replay the id-resolution: id_override beats derive_review_question_id_for_artifact.
        let derived = derive_review_question_id_for_artifact(
            "plan",
            "p1",
            1,
            "compile",
            Some("/some/file"),
        );
        let id_override = "review:custom:override";
        let chosen = if !id_override.trim().is_empty() {
            id_override.to_string()
        } else {
            derived.clone()
        };
        assert_eq!(chosen, "review:custom:override");
        assert_ne!(chosen, derived);
    }

    #[test]
    fn review_gate_policy_was_explicit_detects_presence() {
        assert!(!review_gate_policy_was_explicit(&json!({})));
        assert!(!review_gate_policy_was_explicit(
            &json!({"emit_review_question": true})
        ));
        // Even an empty / unknown value still counts as "the key was sent",
        // so the response should stamp `review_gate_policy=manual` to make
        // the resolution visible.
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": ""})
        ));
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": "off"})
        ));
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": "emit_question"})
        ));
    }

    #[test]
    fn auto_emit_decision_variants_are_distinct() {
        // Pinning that the four decision variants are distinct so callers
        // can pattern-match them in tests / logging without surprise.
        assert_ne!(
            AutoEmitDecision::SkippedPolicyManual,
            AutoEmitDecision::SkippedPolicyOff
        );
        assert_ne!(
            AutoEmitDecision::SkippedFileWriteUnsuccessful,
            AutoEmitDecision::Emitted
        );
        assert_ne!(
            AutoEmitDecision::Emitted,
            AutoEmitDecision::EmitFailedBus
        );
    }

    // ── wave-15 :: explicit review-resolution input ─────────────────────

    #[test]
    fn decision_parse_accepts_canonical_strings() {
        assert_eq!(ReviewDecision::parse("approved").unwrap(), ReviewDecision::Approved);
        assert_eq!(ReviewDecision::parse("rejected").unwrap(), ReviewDecision::Rejected);
        assert_eq!(
            ReviewDecision::parse("needs_changes").unwrap(),
            ReviewDecision::NeedsChanges
        );
    }

    #[test]
    fn decision_parse_is_case_insensitive_and_trims() {
        assert_eq!(ReviewDecision::parse("  Approved  ").unwrap(), ReviewDecision::Approved);
        assert_eq!(ReviewDecision::parse("REJECTED").unwrap(), ReviewDecision::Rejected);
        assert_eq!(
            ReviewDecision::parse("Needs-Changes").unwrap(),
            ReviewDecision::NeedsChanges
        );
    }

    #[test]
    fn decision_parse_accepts_short_aliases() {
        assert_eq!(ReviewDecision::parse("approve").unwrap(), ReviewDecision::Approved);
        assert_eq!(ReviewDecision::parse("reject").unwrap(), ReviewDecision::Rejected);
        assert_eq!(ReviewDecision::parse("changes").unwrap(), ReviewDecision::NeedsChanges);
    }

    #[test]
    fn decision_parse_rejects_unknown() {
        let err = ReviewDecision::parse("approved-with-comments").unwrap_err();
        assert!(matches!(err, ResolutionInputError::UnknownDecision(_)));
        assert_eq!(err.code(), "INVALID_PARAM");
        assert!(err.message().contains("approved-with-comments"));
    }

    #[test]
    fn decision_outcome_mapping_is_total() {
        assert_eq!(
            ReviewDecision::Approved.outcome(),
            ResolutionOutcome::PerformTransition
        );
        assert_eq!(
            ReviewDecision::Rejected.outcome(),
            ResolutionOutcome::KeepArtifact
        );
        assert_eq!(
            ReviewDecision::NeedsChanges.outcome(),
            ResolutionOutcome::RequestChanges
        );
    }

    #[test]
    fn decision_label_round_trips() {
        assert_eq!(ReviewDecision::Approved.as_str(), "approved");
        assert_eq!(ReviewDecision::Rejected.as_str(), "rejected");
        assert_eq!(ReviewDecision::NeedsChanges.as_str(), "needs_changes");
    }

    #[test]
    fn parse_resolution_input_returns_none_when_qid_absent() {
        let out = parse_review_resolution_input(&json!({})).unwrap();
        assert!(out.is_none());
        // Even with a decision present, no qid → quiet path.
        let out = parse_review_resolution_input(&json!({"review_decision": "approved"})).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn parse_resolution_input_full_shape() {
        let out = parse_review_resolution_input(&json!({
            "review_question_id": "review:directive:abc:v1:approve",
            "review_decision": "approved",
            "review_actor": "  alice  ",
            "review_note": "  ship it  ",
        }))
        .unwrap()
        .expect("full input present");
        assert_eq!(out.question_id, "review:directive:abc:v1:approve");
        assert_eq!(out.decision, ReviewDecision::Approved);
        assert_eq!(out.actor.as_deref(), Some("alice"));
        assert_eq!(out.note.as_deref(), Some("ship it"));
    }

    #[test]
    fn parse_resolution_input_missing_decision_fails_fast() {
        let err = parse_review_resolution_input(&json!({
            "review_question_id": "review:directive:abc:v1:approve",
        }))
        .unwrap_err();
        assert_eq!(err, ResolutionInputError::MissingDecision);
        assert_eq!(err.code(), "MISSING_PARAM");
        assert!(err.message().contains("review_decision"));
    }

    #[test]
    fn parse_resolution_input_unknown_decision_fails_fast() {
        let err = parse_review_resolution_input(&json!({
            "review_question_id": "review:plan:p1:v1:approve",
            "review_decision": "looks_good",
        }))
        .unwrap_err();
        assert!(matches!(err, ResolutionInputError::UnknownDecision(_)));
        assert_eq!(err.code(), "INVALID_PARAM");
    }

    #[test]
    fn parse_resolution_input_blank_strings_collapse_to_none_for_actor_note() {
        let out = parse_review_resolution_input(&json!({
            "review_question_id": "review:plan:p1:v1:approve",
            "review_decision": "approved",
            "review_actor": "   ",
            "review_note": "",
        }))
        .unwrap()
        .unwrap();
        assert!(out.actor.is_none());
        assert!(out.note.is_none());
    }

    // ── wave-15 :: deterministic id parser ──────────────────────────────

    #[test]
    fn parse_qid_legacy_layout_no_topic_hash() {
        let p = parse_review_question_id_struct("review:directive:abc-123:v1:compile").unwrap();
        assert_eq!(p.scope, "directive");
        assert_eq!(p.artifact_id, "abc-123");
        assert_eq!(p.version, 1);
        assert_eq!(p.action, "compile");
        assert!(p.topic_hash.is_none());
    }

    #[test]
    fn parse_qid_with_topic_hash_layout() {
        let p =
            parse_review_question_id_struct("review:plan:p-7:v3:compile:abcdef0123456789").unwrap();
        assert_eq!(p.scope, "plan");
        assert_eq!(p.artifact_id, "p-7");
        assert_eq!(p.version, 3);
        assert_eq!(p.action, "compile");
        assert_eq!(p.topic_hash.as_deref(), Some("abcdef0123456789"));
    }

    #[test]
    fn parse_qid_round_trips_against_derive() {
        let original = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            7,
            "compile",
            Some("topic-foo"),
        );
        let p = parse_review_question_id_struct(&original).unwrap();
        assert_eq!(p.scope, "directive");
        assert_eq!(p.artifact_id, "abc");
        assert_eq!(p.version, 7);
        assert_eq!(p.action, "compile");
        assert!(p.topic_hash.is_some());
    }

    #[test]
    fn parse_qid_lowercases_action_for_match() {
        let p = parse_review_question_id_struct("review:directive:abc:v1:Approve").unwrap();
        assert_eq!(p.action, "approve");
    }

    #[test]
    fn parse_qid_rejects_missing_prefix() {
        let err = parse_review_question_id_struct("directive:abc:v1:compile").unwrap_err();
        assert_eq!(err, ReviewIdParseError::MissingPrefix);
    }

    #[test]
    fn parse_qid_rejects_too_few_segments() {
        let err = parse_review_question_id_struct("review:directive:abc:v1").unwrap_err();
        assert_eq!(err, ReviewIdParseError::InsufficientSegments);
    }

    #[test]
    fn parse_qid_rejects_too_many_segments() {
        let err = parse_review_question_id_struct(
            "review:directive:abc:v1:compile:topic-hash:extra-trailing",
        )
        .unwrap_err();
        assert_eq!(err, ReviewIdParseError::InsufficientSegments);
    }

    #[test]
    fn parse_qid_rejects_empty_segments() {
        assert_eq!(
            parse_review_question_id_struct("review::abc:v1:compile").unwrap_err(),
            ReviewIdParseError::EmptySegment("scope")
        );
        assert_eq!(
            parse_review_question_id_struct("review:directive::v1:compile").unwrap_err(),
            ReviewIdParseError::EmptySegment("artifact_id")
        );
        assert_eq!(
            parse_review_question_id_struct("review:directive:abc:v1:").unwrap_err(),
            ReviewIdParseError::EmptySegment("action")
        );
        assert_eq!(
            parse_review_question_id_struct("review:directive:abc:v1:compile:").unwrap_err(),
            ReviewIdParseError::EmptySegment("topic_hash")
        );
    }

    #[test]
    fn parse_qid_rejects_bad_version_segment() {
        let err = parse_review_question_id_struct("review:directive:abc:1:compile").unwrap_err();
        assert!(matches!(err, ReviewIdParseError::BadVersion(_)));
        let err = parse_review_question_id_struct("review:directive:abc:vNaN:compile").unwrap_err();
        assert!(matches!(err, ReviewIdParseError::BadVersion(_)));
    }

    // ── wave-15 :: validate_review_resolution_envelope ──────────────────

    fn make_parsed(scope: &str, id: &str, version: i32, action: &str) -> ParsedReviewQuestionId {
        ParsedReviewQuestionId {
            scope: scope.to_string(),
            artifact_id: id.to_string(),
            version,
            action: action.to_string(),
            topic_hash: None,
        }
    }

    #[test]
    fn validate_envelope_accepts_matching_directive_approve() {
        let parsed = make_parsed("directive", "abc", 1, "approve");
        validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            1,
            &["compile", "approve", "archive"],
        )
        .expect("happy path must succeed");
    }

    #[test]
    fn validate_envelope_rejects_scope_mismatch() {
        // qid says `plan` but submitted to directive surface.
        let parsed = make_parsed("plan", "abc", 1, "approve");
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            1,
            &["compile", "approve", "archive"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_SCOPE_MISMATCH");
    }

    #[test]
    fn validate_envelope_rejects_unsupported_scope() {
        let parsed = make_parsed("worker", "abc", 1, "approve");
        let err = validate_review_resolution_envelope(
            &parsed,
            "worker",
            "abc",
            1,
            &["approve"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_SCOPE_UNSUPPORTED");
    }

    #[test]
    fn validate_envelope_rejects_artifact_id_mismatch() {
        let parsed = make_parsed("directive", "xyz", 1, "approve");
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            1,
            &["approve"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ARTIFACT_MISMATCH");
    }

    #[test]
    fn validate_envelope_rejects_stale_version() {
        // qid says v1 but artifact is at v2.
        let parsed = make_parsed("directive", "abc", 1, "approve");
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            2,
            &["approve"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "STALE_REVIEW_VERSION");
        assert!(err.message().contains("v1"));
        assert!(err.message().contains("v2"));
    }

    #[test]
    fn validate_envelope_rejects_unsupported_action() {
        let parsed = make_parsed("directive", "abc", 1, "supersede");
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            1,
            &["compile", "approve", "archive"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ACTION_UNSUPPORTED");
        assert!(err.message().contains("supersede"));
    }

    // ── wave-15 :: payload stamping ─────────────────────────────────────

    fn approved_input() -> ReviewResolutionInput {
        ReviewResolutionInput {
            question_id: "review:directive:abc:v1:approve".to_string(),
            decision: ReviewDecision::Approved,
            actor: Some("alice".to_string()),
            note: Some("ship it".to_string()),
        }
    }

    #[test]
    fn stamp_resolution_payload_includes_decision_outcome_actor_note() {
        let mut p = json!({"status": "approved"});
        stamp_resolution_payload(&mut p, &approved_input());
        assert_eq!(p["review_question_id"], "review:directive:abc:v1:approve");
        assert_eq!(p["review_decision"], "approved");
        assert_eq!(p["review_decision_outcome"], "perform_transition");
        assert_eq!(p["review_actor"], "alice");
        assert_eq!(p["review_note"], "ship it");
    }

    #[test]
    fn stamp_resolution_payload_omits_actor_note_when_absent() {
        let mut p = json!({"status": "rejected"});
        let input = ReviewResolutionInput {
            question_id: "review:plan:p1:v1:approve".to_string(),
            decision: ReviewDecision::Rejected,
            actor: None,
            note: None,
        };
        stamp_resolution_payload(&mut p, &input);
        assert_eq!(p["review_decision"], "rejected");
        assert_eq!(p["review_decision_outcome"], "keep_artifact");
        assert!(p.get("review_actor").is_none());
        assert!(p.get("review_note").is_none());
    }

    #[test]
    fn stamp_needs_changes_next_step_is_actionable() {
        let mut p = json!({"status": "review"});
        stamp_needs_changes_next_step(&mut p, "directive", "compile");
        let next = p["next_step"].as_str().unwrap();
        assert!(next.contains("rework"));
        assert!(next.contains("directive"));
        assert!(next.contains("compile"));
    }

    #[test]
    fn resolution_wire_string_matches_decision_label() {
        assert_eq!(resolution_wire_string(ReviewDecision::Approved), "approved");
        assert_eq!(resolution_wire_string(ReviewDecision::Rejected), "rejected");
        assert_eq!(
            resolution_wire_string(ReviewDecision::NeedsChanges),
            "needs_changes"
        );
    }

    #[test]
    fn resolution_outcome_variants_distinct() {
        assert_ne!(
            ResolutionOutcome::PerformTransition,
            ResolutionOutcome::KeepArtifact
        );
        assert_ne!(
            ResolutionOutcome::KeepArtifact,
            ResolutionOutcome::RequestChanges
        );
    }

    // ── wave-16 :: subscriber-side resolution dispatcher ────────────────

    #[test]
    fn subscriber_resolution_string_approve_synonyms_collapse_to_approved() {
        for raw in ["approved", "approve", "yes", "accepted", "Approved", "  YES  "] {
            assert_eq!(
                parse_subscriber_resolution_string(raw),
                Some(ReviewDecision::Approved),
                "expected Approved for `{}`",
                raw
            );
        }
    }

    #[test]
    fn subscriber_resolution_string_reject_synonyms_collapse_to_rejected() {
        for raw in ["rejected", "reject", "no", "Reject", " NO "] {
            assert_eq!(
                parse_subscriber_resolution_string(raw),
                Some(ReviewDecision::Rejected),
                "expected Rejected for `{}`",
                raw
            );
        }
    }

    #[test]
    fn subscriber_resolution_string_needs_changes_synonyms_collapse() {
        for raw in [
            "needs_changes",
            "needs-changes",
            "changes",
            "revise",
            "fix",
            "Revise",
            "  FIX  ",
        ] {
            assert_eq!(
                parse_subscriber_resolution_string(raw),
                Some(ReviewDecision::NeedsChanges),
                "expected NeedsChanges for `{}`",
                raw
            );
        }
    }

    #[test]
    fn subscriber_resolution_string_unknown_returns_none() {
        for raw in ["", "maybe", "deferred", "unsure", "abstain"] {
            assert!(
                parse_subscriber_resolution_string(raw).is_none(),
                "expected None for `{}`",
                raw
            );
        }
    }

    #[test]
    fn dispatch_ignores_non_review_id() {
        let d = plan_review_resolved_dispatch("master:abc:approve", "approved");
        assert_eq!(d, ReviewResolvedDispatch::IgnoreNonReviewId);
    }

    #[test]
    fn dispatch_ignores_blank_id_as_non_review() {
        let d = plan_review_resolved_dispatch("", "approved");
        assert_eq!(d, ReviewResolvedDispatch::IgnoreNonReviewId);
    }

    #[test]
    fn dispatch_ignores_malformed_review_id() {
        let d = plan_review_resolved_dispatch("review:directive", "approved");
        match d {
            ReviewResolvedDispatch::IgnoreMalformedId(_) => {}
            other => panic!("expected IgnoreMalformedId, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_ignores_unsupported_scope_even_when_id_well_formed() {
        // `chat` is not directive/plan/workflow → defensive ignore.
        let d = plan_review_resolved_dispatch("review:chat:abc:v1:approve", "approved");
        match d {
            ReviewResolvedDispatch::IgnoreUnsupportedScope { scope } => {
                assert_eq!(scope, "chat");
            }
            other => panic!("expected IgnoreUnsupportedScope, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_ignores_unknown_resolution() {
        let d = plan_review_resolved_dispatch(
            "review:directive:abc:v1:approve",
            "deferred",
        );
        match d {
            ReviewResolvedDispatch::IgnoreUnknownResolution { resolution } => {
                assert_eq!(resolution, "deferred");
            }
            other => panic!("expected IgnoreUnknownResolution, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_routes_directive_approved() {
        let d = plan_review_resolved_dispatch(
            "review:directive:abc-123:v1:approve",
            "approved",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "directive");
                assert_eq!(parsed.artifact_id, "abc-123");
                assert_eq!(parsed.version, 1);
                assert_eq!(parsed.action, "approve");
                assert_eq!(decision, ReviewDecision::Approved);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_routes_plan_rejected_via_synonym() {
        let d = plan_review_resolved_dispatch(
            "review:plan:9f3c:v2:supersede",
            "no",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "plan");
                assert_eq!(parsed.artifact_id, "9f3c");
                assert_eq!(parsed.version, 2);
                assert_eq!(parsed.action, "supersede");
                assert_eq!(decision, ReviewDecision::Rejected);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_routes_workflow_needs_changes_via_synonym() {
        let d = plan_review_resolved_dispatch(
            "review:workflow:methodology-deploy-v0:v1:compile",
            "fix",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "workflow");
                assert_eq!(parsed.artifact_id, "methodology-deploy-v0");
                assert_eq!(parsed.version, 1);
                assert_eq!(parsed.action, "compile");
                assert_eq!(decision, ReviewDecision::NeedsChanges);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_routes_with_topic_hash_suffix() {
        let d = plan_review_resolved_dispatch(
            "review:directive:abc:v3:compile:0123456789abcdef",
            "approve",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "directive");
                assert_eq!(parsed.action, "compile");
                assert_eq!(
                    parsed.topic_hash.as_deref(),
                    Some("0123456789abcdef")
                );
                assert_eq!(decision, ReviewDecision::Approved);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    // ── wave-17 / task 01 — plan-node resume helpers ──────────────────

    #[test]
    fn derive_plan_node_topic_hash_matches_emitter_round_trip() {
        // The hash the resume helper extracts MUST equal the hash the
        // wave-16 / task 04 pause emitter folds into the deterministic id —
        // otherwise the resume listener can never map an inbound qid back
        // to its originating paused node id.
        let plan_id = "00000000-0000-0000-0000-000000000abc";
        let qid = derive_plan_node_review_question_id(plan_id, 1, "node-g", None);
        let parsed = parse_review_question_id_struct(&qid).expect("valid envelope");
        let hash = derive_plan_node_topic_hash("node-g");
        assert_eq!(parsed.topic_hash.as_deref(), Some(hash.as_str()));
        // Hash length is the wave-14 contract: 16 hex chars.
        assert_eq!(hash.len(), 16);
    }

    #[test]
    fn derive_plan_node_topic_hash_is_deterministic_per_node_id() {
        // Same node id always hashes to the same prefix so the resume
        // helper's lookup is stable across daemon restarts.
        let a = derive_plan_node_topic_hash("alpha");
        let b = derive_plan_node_topic_hash("alpha");
        assert_eq!(a, b);
        assert_ne!(a, derive_plan_node_topic_hash("beta"));
    }

    #[test]
    fn is_plan_node_review_action_matches_default_action_case_insensitive() {
        assert!(is_plan_node_review_action("plan-node"));
        assert!(is_plan_node_review_action("PLAN-NODE"));
        assert!(is_plan_node_review_action("  plan-node  "));
        assert!(!is_plan_node_review_action("compile"));
        assert!(!is_plan_node_review_action("approve"));
        assert!(!is_plan_node_review_action(""));
    }

    // ── wave-17 / task 01 — resume input parser ───────────────────────

    #[test]
    fn parse_plan_node_resume_input_returns_none_when_id_absent() {
        // Quiet path: no `resume_review_question_id` → caller falls
        // through to the standard execute pipeline. Must NOT error
        // because absence is the legacy-quiet contract.
        assert!(parse_plan_node_resume_input(&json!({})).expect("ok").is_none());
        assert!(parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "   "
        }))
        .expect("ok")
        .is_none());
    }

    #[test]
    fn parse_plan_node_resume_input_extracts_full_envelope() {
        let input = parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "  review:plan:abc:v1:plan-node:0123456789abcdef  ",
            "resume_review_decision": "approved",
            "resume_actor": "  agent-team  ",
            "resume_note": "  proceed  ",
        }))
        .expect("ok")
        .expect("some");
        assert_eq!(
            input.question_id,
            "review:plan:abc:v1:plan-node:0123456789abcdef"
        );
        assert_eq!(input.decision, ReviewDecision::Approved);
        assert_eq!(input.actor.as_deref(), Some("agent-team"));
        assert_eq!(input.note.as_deref(), Some("proceed"));
    }

    #[test]
    fn parse_plan_node_resume_input_parses_rejected_and_needs_changes() {
        for (raw, expected) in [
            ("rejected", ReviewDecision::Rejected),
            ("needs_changes", ReviewDecision::NeedsChanges),
            ("REJECTED", ReviewDecision::Rejected),
            ("changes", ReviewDecision::NeedsChanges),
        ] {
            let input = parse_plan_node_resume_input(&json!({
                "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
                "resume_review_decision": raw,
            }))
            .expect("ok")
            .expect("some");
            assert_eq!(input.decision, expected, "decision raw={}", raw);
        }
    }

    #[test]
    fn parse_plan_node_resume_input_id_without_decision_is_missing_decision_error() {
        // The id is load-bearing — supplying it without a decision is
        // fail-fast (mirrors the wave-15 manager-side parser behaviour).
        let err = parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
        }))
        .expect_err("missing decision");
        assert_eq!(err, ResolutionInputError::MissingDecision);
    }

    #[test]
    fn parse_plan_node_resume_input_unknown_decision_is_unknown_decision_error() {
        let err = parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
            "resume_review_decision": "looks-good-to-me",
        }))
        .expect_err("unknown decision");
        match err {
            ResolutionInputError::UnknownDecision(raw) => {
                assert_eq!(raw, "looks-good-to-me");
            }
            other => panic!("expected UnknownDecision, got {:?}", other),
        }
    }

    #[test]
    fn parse_plan_node_resume_input_filters_blank_actor_and_note() {
        let input = parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
            "resume_review_decision": "approved",
            "resume_actor": "   ",
            "resume_note": "",
        }))
        .expect("ok")
        .expect("some");
        assert!(input.actor.is_none());
        assert!(input.note.is_none());
    }

    // ── wave-18 / task 07 — review automation policy ────────────────────

    #[test]
    fn parse_automation_policy_default_is_manual() {
        assert_eq!(
            parse_review_automation_policy(&json!({})),
            ReviewAutomationPolicy::Manual
        );
    }

    #[test]
    fn parse_automation_policy_recognises_suggest() {
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "suggest"})),
            ReviewAutomationPolicy::Suggest
        );
    }

    #[test]
    fn parse_automation_policy_recognises_auto_safe() {
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "auto_safe"})),
            ReviewAutomationPolicy::AutoSafe
        );
        // Hyphenated alias accepted to keep authoring typos loud only on
        // truly unknown values.
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "auto-safe"})),
            ReviewAutomationPolicy::AutoSafe
        );
    }

    #[test]
    fn parse_automation_policy_is_case_insensitive_and_trims() {
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "  AUTO_SAFE  "})),
            ReviewAutomationPolicy::AutoSafe
        );
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "Suggest"})),
            ReviewAutomationPolicy::Suggest
        );
    }

    #[test]
    fn parse_automation_policy_unknown_collapses_to_manual() {
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "yolo"})),
            ReviewAutomationPolicy::Manual
        );
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": ""})),
            ReviewAutomationPolicy::Manual
        );
    }

    #[test]
    fn automation_policy_was_explicit_detects_presence() {
        assert!(!review_automation_policy_was_explicit(&json!({})));
        assert!(review_automation_policy_was_explicit(
            &json!({"review_automation_policy": ""})
        ));
        assert!(review_automation_policy_was_explicit(
            &json!({"review_automation_policy": "auto_safe"})
        ));
    }

    #[test]
    fn automation_policy_label_round_trips() {
        assert_eq!(ReviewAutomationPolicy::Manual.as_str(), "manual");
        assert_eq!(ReviewAutomationPolicy::Suggest.as_str(), "suggest");
        assert_eq!(ReviewAutomationPolicy::AutoSafe.as_str(), "auto_safe");
    }

    #[test]
    fn automation_status_label_round_trips() {
        assert_eq!(AutomationStatus::NotEvaluated.as_str(), "not_evaluated");
        assert_eq!(AutomationStatus::Suggested.as_str(), "suggested");
        assert_eq!(AutomationStatus::AutoApproved.as_str(), "auto_approved");
        assert_eq!(
            AutomationStatus::AutoSafeBlocked.as_str(),
            "auto_safe_blocked"
        );
        assert_eq!(
            AutomationStatus::OverriddenByExplicitDecision.as_str(),
            "overridden_by_explicit_decision"
        );
    }

    fn safe_ctx() -> ReviewAutomationContext {
        ReviewAutomationContext {
            deterministic_mode: true,
            file_write_attempted: true,
            file_write_succeeded: true,
            actual_file_sha256: Some("deadbeef".repeat(8)),
            expected_file_sha256: Some("deadbeef".repeat(8)),
            protected_source_or_target: false,
            additional_blockers: Vec::new(),
        }
    }

    #[test]
    fn evaluate_manual_returns_not_evaluated_with_empty_block() {
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::Manual,
            &safe_ctx(),
            None,
        );
        assert_eq!(outcome.policy, ReviewAutomationPolicy::Manual);
        assert_eq!(outcome.status, AutomationStatus::NotEvaluated);
        assert!(outcome.suggested_decision.is_none());
        assert!(outcome.reasons.is_empty());
        assert!(!outcome.may_auto_resolve);
    }

    #[test]
    fn evaluate_suggest_returns_suggestion_without_mutation() {
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::Suggest,
            &safe_ctx(),
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::Suggested);
        assert_eq!(outcome.suggested_decision, Some(ReviewDecision::Approved));
        assert!(!outcome.may_auto_resolve);
        assert!(!outcome.reasons.is_empty());
    }

    #[test]
    fn evaluate_auto_safe_approves_when_every_rule_passes() {
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &safe_ctx(),
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoApproved);
        assert_eq!(outcome.suggested_decision, Some(ReviewDecision::Approved));
        assert!(outcome.may_auto_resolve);
        // Only passing reasons survive on the AutoApproved path.
        for r in &outcome.reasons {
            assert!(
                r.starts_with("rule:"),
                "reason `{}` should start with `rule:`",
                r
            );
        }
    }

    #[test]
    fn evaluate_auto_safe_blocked_when_protected_source_or_target() {
        let mut ctx = safe_ctx();
        ctx.protected_source_or_target = true;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(!outcome.may_auto_resolve);
        assert_eq!(
            outcome.suggested_decision,
            Some(ReviewDecision::NeedsChanges)
        );
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("protected_source_or_target")));
    }

    #[test]
    fn evaluate_auto_safe_blocked_when_non_deterministic() {
        let mut ctx = safe_ctx();
        ctx.deterministic_mode = false;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(!outcome.may_auto_resolve);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("deterministic_mode")));
    }

    #[test]
    fn evaluate_auto_safe_blocked_when_file_hash_mismatch() {
        let mut ctx = safe_ctx();
        ctx.actual_file_sha256 = Some("aaaa".repeat(8));
        ctx.expected_file_sha256 = Some("bbbb".repeat(8));
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(!outcome.may_auto_resolve);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("file_hash_mismatch")));
    }

    #[test]
    fn evaluate_auto_safe_blocked_when_file_write_failed() {
        let mut ctx = safe_ctx();
        ctx.file_write_attempted = true;
        ctx.file_write_succeeded = false;
        ctx.actual_file_sha256 = None;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("file_write_unsuccessful")));
    }

    #[test]
    fn evaluate_auto_safe_passes_when_no_file_write_attempted() {
        let mut ctx = safe_ctx();
        ctx.file_write_attempted = false;
        ctx.file_write_succeeded = false;
        ctx.actual_file_sha256 = None;
        ctx.expected_file_sha256 = None;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        // No file write attempted → the no-file-write rule auto-passes.
        assert_eq!(outcome.status, AutomationStatus::AutoApproved);
        assert!(outcome.may_auto_resolve);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("no_file_write")));
    }

    #[test]
    fn evaluate_auto_safe_blocked_by_additional_blocker() {
        let mut ctx = safe_ctx();
        ctx.additional_blockers
            .push("status=partial: review_question_warning present".to_string());
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("additional_blocker")));
    }

    #[test]
    fn evaluate_explicit_decision_overrides_suggestion() {
        // Even when every rule passes, an explicit caller decision wins.
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &safe_ctx(),
            Some(ReviewDecision::Rejected),
        );
        assert_eq!(
            outcome.status,
            AutomationStatus::OverriddenByExplicitDecision
        );
        // We still surface the suggestion for audit, but never mutate.
        assert_eq!(outcome.suggested_decision, Some(ReviewDecision::Approved));
        assert!(!outcome.may_auto_resolve);
    }

    #[test]
    fn evaluate_explicit_decision_overrides_under_suggest_too() {
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::Suggest,
            &safe_ctx(),
            Some(ReviewDecision::Approved),
        );
        assert_eq!(
            outcome.status,
            AutomationStatus::OverriddenByExplicitDecision
        );
        assert!(!outcome.may_auto_resolve);
    }

    #[test]
    fn evaluate_auto_safe_never_auto_rejects_even_when_suggestion_is_needs_changes() {
        // A blocking rule degrades the suggestion to NeedsChanges; even
        // though the suggestion is unanimous, auto_safe NEVER mutates
        // toward rejection / needs_changes — it only auto-approves.
        let mut ctx = safe_ctx();
        ctx.protected_source_or_target = true;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert!(!outcome.may_auto_resolve);
        assert_eq!(
            outcome.suggested_decision,
            Some(ReviewDecision::NeedsChanges)
        );
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("auto_safe_refuses_non_approved")
                || r.contains("protected_source_or_target")));
    }

    #[test]
    fn stamp_automation_payload_under_manual_is_emitted_when_called() {
        // The handler is responsible for skipping the call under Manual to
        // keep pre-wave-18 callers byte-identical. But if it IS called
        // (e.g. from a future explicit-status path), the stamp shape stays
        // sane — `suggested_review_decision` is omitted.
        let mut p = json!({"status": "approved"});
        let outcome = ReviewAutomationOutcome {
            policy: ReviewAutomationPolicy::Manual,
            status: AutomationStatus::NotEvaluated,
            suggested_decision: None,
            reasons: Vec::new(),
            may_auto_resolve: false,
        };
        stamp_review_automation_payload(&mut p, &outcome);
        assert_eq!(p["review_automation_policy"], "manual");
        assert_eq!(p["review_automation_status"], "not_evaluated");
        assert!(p.get("suggested_review_decision").is_none());
        assert_eq!(p["automation_reasons"], json!([]));
    }

    #[test]
    fn stamp_automation_payload_includes_suggestion_under_suggest() {
        let mut p = json!({"status": "draft"});
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::Suggest,
            &safe_ctx(),
            None,
        );
        stamp_review_automation_payload(&mut p, &outcome);
        assert_eq!(p["review_automation_policy"], "suggest");
        assert_eq!(p["review_automation_status"], "suggested");
        assert_eq!(p["suggested_review_decision"], "approved");
        assert!(p["automation_reasons"].is_array());
        assert!(!p["automation_reasons"].as_array().unwrap().is_empty());
    }

    #[test]
    fn stamp_automation_payload_under_auto_approved_path() {
        let mut p = json!({"status": "approved"});
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &safe_ctx(),
            None,
        );
        stamp_review_automation_payload(&mut p, &outcome);
        assert_eq!(p["review_automation_policy"], "auto_safe");
        assert_eq!(p["review_automation_status"], "auto_approved");
        assert_eq!(p["suggested_review_decision"], "approved");
    }

    // ── wave-20 / task 08 — review auto-answer policy ────────────────────

    #[test]
    fn parse_auto_answer_policy_default_is_off() {
        assert_eq!(
            parse_auto_answer_policy(&json!({})),
            AutoAnswerPolicy::Off
        );
    }

    #[test]
    fn parse_auto_answer_policy_recognises_deterministic_safe() {
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "deterministic_safe"})),
            AutoAnswerPolicy::DeterministicSafe
        );
        // Hyphenated alias accepted to keep authoring typos loud only on
        // truly unknown values.
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "deterministic-safe"})),
            AutoAnswerPolicy::DeterministicSafe
        );
    }

    #[test]
    fn parse_auto_answer_policy_recognises_dry_run() {
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "dry_run"})),
            AutoAnswerPolicy::DryRun
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "dry-run"})),
            AutoAnswerPolicy::DryRun
        );
    }

    #[test]
    fn parse_auto_answer_policy_is_case_insensitive_and_trims() {
        assert_eq!(
            parse_auto_answer_policy(
                &json!({"auto_answer_policy": "  DETERMINISTIC_SAFE  "})
            ),
            AutoAnswerPolicy::DeterministicSafe
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "Dry_Run"})),
            AutoAnswerPolicy::DryRun
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "OFF"})),
            AutoAnswerPolicy::Off
        );
    }

    #[test]
    fn parse_auto_answer_policy_unknown_collapses_to_off() {
        // Unknown values silently map to the default rather than rejected
        // — the response always echoes the resolved policy so a typo is
        // observable downstream.
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "auto_approve"})),
            AutoAnswerPolicy::Off
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": ""})),
            AutoAnswerPolicy::Off
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "   "})),
            AutoAnswerPolicy::Off
        );
    }

    #[test]
    fn auto_answer_policy_was_explicit_detects_presence() {
        assert!(!auto_answer_policy_was_explicit(&json!({})));
        assert!(auto_answer_policy_was_explicit(
            &json!({"auto_answer_policy": ""})
        ));
        assert!(auto_answer_policy_was_explicit(
            &json!({"auto_answer_policy": "dry_run"})
        ));
        assert!(auto_answer_policy_was_explicit(
            &json!({"auto_answer_policy": "off"})
        ));
    }

    #[test]
    fn auto_answer_policy_label_round_trips() {
        assert_eq!(AutoAnswerPolicy::Off.as_str(), "off");
        assert_eq!(
            AutoAnswerPolicy::DeterministicSafe.as_str(),
            "deterministic_safe"
        );
        assert_eq!(AutoAnswerPolicy::DryRun.as_str(), "dry_run");
    }

    #[test]
    fn auto_answer_status_label_round_trips() {
        assert_eq!(AutoAnswerStatus::NotEvaluated.as_str(), "not_evaluated");
        assert_eq!(AutoAnswerStatus::AutoAnswered.as_str(), "auto_answered");
        assert_eq!(
            AutoAnswerStatus::SkippedRulesFailed.as_str(),
            "skipped_rules_failed"
        );
        assert_eq!(
            AutoAnswerStatus::SkippedDestructiveAction.as_str(),
            "skipped_destructive_action"
        );
        assert_eq!(
            AutoAnswerStatus::DryRunPreview.as_str(),
            "dry_run_preview"
        );
    }

    // ── wave-20 / task 08 — destructive-action guard ─────────────────────

    #[test]
    fn destructive_action_recognises_archive_supersede_remove() {
        for raw in [
            "archive",
            "supersede",
            "remove",
            "Archive",
            "SUPERSEDE",
            "  Remove  ",
        ] {
            assert!(
                is_destructive_review_action(raw),
                "expected destructive for `{}`",
                raw
            );
        }
    }

    #[test]
    fn non_destructive_actions_are_safe() {
        for raw in [
            "compile",
            "approve",
            "mark",
            "plan-node",
            "human-checkpoint",
            "",
            "  ",
        ] {
            assert!(
                !is_destructive_review_action(raw),
                "expected non-destructive for `{}`",
                raw
            );
        }
    }

    // ── wave-20 / task 08 — evaluate_auto_answer_policy ──────────────────

    #[test]
    fn evaluate_auto_answer_off_returns_not_evaluated_with_empty_block() {
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::Off,
            &safe_ctx(),
            "approve",
            None,
        );
        assert_eq!(outcome.policy, AutoAnswerPolicy::Off);
        assert_eq!(outcome.status, AutoAnswerStatus::NotEvaluated);
        assert!(outcome.selected_decision.is_none());
        assert!(outcome.safety_rule_results.is_empty());
        // Off mode does NOT defer — caller routes the inbound decision
        // unchanged. requires_human=false matches the byte-identical
        // pre-wave-20/08 contract.
        assert!(!outcome.requires_human);
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_auto_answers_when_every_rule_passes() {
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::AutoAnswered);
        assert_eq!(outcome.selected_decision, Some(ReviewDecision::Approved));
        assert!(!outcome.requires_human);
        // The destructive-action rule surfaces even on the happy path so
        // observers see why the action was eligible.
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("non_destructive_action")));
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocks_destructive_archive() {
        // Even when every other rule passes, archive MUST defer.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "archive",
            None,
        );
        assert_eq!(
            outcome.status,
            AutoAnswerStatus::SkippedDestructiveAction
        );
        assert!(outcome.requires_human);
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("destructive_action") && r.contains("archive")));
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocks_destructive_supersede() {
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "supersede",
            None,
        );
        assert_eq!(
            outcome.status,
            AutoAnswerStatus::SkippedDestructiveAction
        );
        assert!(outcome.requires_human);
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocks_destructive_remove() {
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "remove",
            None,
        );
        assert_eq!(
            outcome.status,
            AutoAnswerStatus::SkippedDestructiveAction
        );
        assert!(outcome.requires_human);
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocked_when_protected_source_target() {
        let mut ctx = safe_ctx();
        ctx.protected_source_or_target = true;
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &ctx,
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::SkippedRulesFailed);
        assert!(outcome.requires_human);
        // Suggestion degraded to NeedsChanges by the upstream inspector
        // because of the protected source/target rule.
        assert_eq!(
            outcome.selected_decision,
            Some(ReviewDecision::NeedsChanges)
        );
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("protected_source_or_target")));
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocked_when_not_deterministic() {
        // sonnet / LLM-driven artefact → wave-18/07 rule trips.
        let mut ctx = safe_ctx();
        ctx.deterministic_mode = false;
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &ctx,
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::SkippedRulesFailed);
        assert!(outcome.requires_human);
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("deterministic_mode")));
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocked_by_additional_blocker() {
        let mut ctx = safe_ctx();
        ctx.additional_blockers
            .push("review_question_warning present".to_string());
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &ctx,
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::SkippedRulesFailed);
        assert!(outcome.requires_human);
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_defers_when_caller_decision_present() {
        // Explicit caller decision wins — the policy NEVER overrides
        // human authority.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            Some(ReviewDecision::Approved),
        );
        assert_eq!(outcome.status, AutoAnswerStatus::SkippedRulesFailed);
        assert!(outcome.requires_human);
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("caller_decision_present")));
    }

    #[test]
    fn evaluate_auto_answer_dry_run_always_defers_even_on_safe_inputs() {
        // dry_run NEVER auto-answers — even when every rule passes the
        // selected_decision is informational and requires_human=true.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DryRun,
            &safe_ctx(),
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::DryRunPreview);
        assert!(outcome.requires_human);
        assert_eq!(outcome.selected_decision, Some(ReviewDecision::Approved));
    }

    #[test]
    fn evaluate_auto_answer_dry_run_preview_for_destructive_still_surfaces_rule() {
        // dry_run preview still surfaces the destructive-action rule on
        // the result block so dashboards see what would have happened.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DryRun,
            &safe_ctx(),
            "supersede",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::DryRunPreview);
        assert!(outcome.requires_human);
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("destructive_action") && r.contains("supersede")));
    }

    #[test]
    fn evaluate_auto_answer_never_returns_rejected_decision_invariant_i1() {
        // Invariant I1: auto-answer NEVER returns Rejected as the
        // selected decision. Even when the upstream inspector degrades
        // the suggestion under blocking rules, we surface NeedsChanges
        // instead of Rejected.
        for ctx_mutation in [
            // Non-deterministic mode trips a blocker.
            |c: &mut ReviewAutomationContext| c.deterministic_mode = false,
            // Protected source/target trips a blocker.
            |c: &mut ReviewAutomationContext| c.protected_source_or_target = true,
            // Hash mismatch trips a blocker.
            |c: &mut ReviewAutomationContext| {
                c.actual_file_sha256 = Some("aaaa".repeat(8));
                c.expected_file_sha256 = Some("bbbb".repeat(8));
            },
        ] {
            let mut ctx = safe_ctx();
            ctx_mutation(&mut ctx);
            for policy in [
                AutoAnswerPolicy::DeterministicSafe,
                AutoAnswerPolicy::DryRun,
            ] {
                let outcome = evaluate_auto_answer_policy(policy, &ctx, "approve", None);
                assert_ne!(
                    outcome.selected_decision,
                    Some(ReviewDecision::Rejected),
                    "invariant I1: auto-answer must NEVER return Rejected (policy={:?})",
                    policy
                );
            }
        }
    }

    #[test]
    fn evaluate_auto_answer_never_promotes_destructive_actions_invariant_i2() {
        // Invariant I2: archive / supersede / remove NEVER auto-promote,
        // even when every safety rule passes. Pinned across both policy
        // modes that evaluate.
        for action in DESTRUCTIVE_REVIEW_ACTIONS {
            // deterministic_safe → SkippedDestructiveAction.
            let outcome = evaluate_auto_answer_policy(
                AutoAnswerPolicy::DeterministicSafe,
                &safe_ctx(),
                action,
                None,
            );
            assert_ne!(
                outcome.status,
                AutoAnswerStatus::AutoAnswered,
                "invariant I2: destructive `{}` must NEVER auto-answer",
                action
            );
            assert!(
                outcome.requires_human,
                "invariant I2: destructive `{}` must require human",
                action
            );
            assert_ne!(
                outcome.selected_decision,
                Some(ReviewDecision::Rejected),
                "invariant I1+I2: destructive `{}` must NEVER auto-reject",
                action
            );

            // dry_run → DryRunPreview (never AutoAnswered) regardless.
            let dry = evaluate_auto_answer_policy(
                AutoAnswerPolicy::DryRun,
                &safe_ctx(),
                action,
                None,
            );
            assert_eq!(dry.status, AutoAnswerStatus::DryRunPreview);
            assert!(dry.requires_human);
        }
    }

    #[test]
    fn evaluate_auto_answer_never_calls_llm_invariant_i3() {
        // Invariant I3: the policy is pure / deterministic / never
        // touches an LLM. We can't directly assert on the absence of a
        // network call, but we CAN pin that the function is sync (no
        // async / no .await) by simply calling it in a sync context.
        // The signature itself enforces this — if a future refactor
        // adds `async fn`, this test fails to compile.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            None,
        );
        // And the decision is deterministic — running twice with the
        // same inputs MUST produce the same output.
        let outcome2 = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            None,
        );
        assert_eq!(outcome, outcome2);
    }

    #[test]
    fn evaluate_auto_answer_skipped_block_carries_full_audit_invariant_i4() {
        // Invariant I4: when skipped (any non-Off mode that did not
        // reach AutoAnswered), the response carries policy_result,
        // selected_decision, safety_rule_results, and requires_human.
        let mut ctx = safe_ctx();
        ctx.protected_source_or_target = true;
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &ctx,
            "approve",
            None,
        );
        // policy_result label set.
        assert_ne!(outcome.status, AutoAnswerStatus::AutoAnswered);
        assert_eq!(outcome.status.as_str(), "skipped_rules_failed");
        // selected_decision present (suggestion degraded to NeedsChanges).
        assert!(outcome.selected_decision.is_some());
        // safety_rule_results non-empty.
        assert!(!outcome.safety_rule_results.is_empty());
        // requires_human=true.
        assert!(outcome.requires_human);
    }

    // ── wave-20 / task 08 — stamp_auto_answer_payload ────────────────────

    #[test]
    fn stamp_auto_answer_payload_under_off_carries_minimal_block() {
        // Helper writes the full block when called even under Off so a
        // future caller that DOES call it sees a well-formed payload.
        // The handler is responsible for skipping the call under Off to
        // keep pre-wave-20/08 callers byte-identical.
        let mut p = json!({"status": "approved"});
        let outcome = AutoAnswerOutcome {
            policy: AutoAnswerPolicy::Off,
            status: AutoAnswerStatus::NotEvaluated,
            selected_decision: None,
            safety_rule_results: Vec::new(),
            requires_human: false,
        };
        stamp_auto_answer_payload(&mut p, &outcome);
        assert_eq!(p["auto_answer_policy"], "off");
        assert_eq!(p["policy_result"], "not_evaluated");
        // No selected_decision when None.
        assert!(p.get("selected_decision").is_none());
        assert_eq!(p["safety_rule_results"], json!([]));
        assert_eq!(p["requires_human"], false);
    }

    #[test]
    fn stamp_auto_answer_payload_under_auto_answered_carries_approved_decision() {
        let mut p = json!({"status": "approved"});
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            None,
        );
        stamp_auto_answer_payload(&mut p, &outcome);
        assert_eq!(p["auto_answer_policy"], "deterministic_safe");
        assert_eq!(p["policy_result"], "auto_answered");
        assert_eq!(p["selected_decision"], "approved");
        assert!(p["safety_rule_results"].is_array());
        assert_eq!(p["requires_human"], false);
    }

    #[test]
    fn stamp_auto_answer_payload_under_skipped_destructive_action() {
        let mut p = json!({"status": "draft"});
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "archive",
            None,
        );
        stamp_auto_answer_payload(&mut p, &outcome);
        assert_eq!(p["auto_answer_policy"], "deterministic_safe");
        assert_eq!(p["policy_result"], "skipped_destructive_action");
        // Suggestion still surfaces (Approved for the safe ctx) even
        // though the listener will defer.
        assert_eq!(p["selected_decision"], "approved");
        assert_eq!(p["requires_human"], true);
        let rules = p["safety_rule_results"].as_array().unwrap();
        assert!(rules
            .iter()
            .any(|r| r.as_str().unwrap().contains("destructive_action")));
    }

    #[test]
    fn stamp_auto_answer_payload_under_dry_run_preview() {
        let mut p = json!({"status": "draft"});
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DryRun,
            &safe_ctx(),
            "approve",
            None,
        );
        stamp_auto_answer_payload(&mut p, &outcome);
        assert_eq!(p["auto_answer_policy"], "dry_run");
        assert_eq!(p["policy_result"], "dry_run_preview");
        assert_eq!(p["selected_decision"], "approved");
        // dry_run ALWAYS defers — pinning the invariant on the stamped
        // shape so a future refactor can't silently flip it.
        assert_eq!(p["requires_human"], true);
    }

    #[test]
    fn stamp_auto_answer_payload_never_emits_rejected_invariant_i1_round_trip() {
        // Belt-and-braces stamping check: even if a synthetic outcome
        // somehow carried Rejected, the stamper should serialise it as
        // the canonical `rejected` label — but the policy CONSTRUCTORS
        // (evaluate_auto_answer_policy) MUST never emit Rejected. Pinning
        // the invariant here as a defensive contract test.
        for ctx_mutation in [
            |c: &mut ReviewAutomationContext| c.deterministic_mode = false,
            |c: &mut ReviewAutomationContext| c.protected_source_or_target = true,
        ] {
            let mut ctx = safe_ctx();
            ctx_mutation(&mut ctx);
            let outcome = evaluate_auto_answer_policy(
                AutoAnswerPolicy::DeterministicSafe,
                &ctx,
                "approve",
                None,
            );
            let mut p = json!({});
            stamp_auto_answer_payload(&mut p, &outcome);
            // selected_decision MUST NOT be `rejected` after the
            // evaluator + stamper round trip.
            if let Some(s) = p.get("selected_decision").and_then(|v| v.as_str()) {
                assert_ne!(
                    s, "rejected",
                    "invariant I1 round-trip: stamper MUST NOT surface `rejected`"
                );
            }
        }
    }

    // ───────────────────────────────────────────────────────────────────
    // wave-21 / task 06 — LLM auto-approve proposal v0
    //
    // Pure helper tests. NO bus, NO DB, NO LLM. The Sonnet call itself
    // is wired in the per-handler integration code; here we pin the
    // parser / invariant / stamper contract.
    // ───────────────────────────────────────────────────────────────────

    fn well_formed_proposal_response() -> &'static str {
        r#"{
            "decision": "approved",
            "confidence": "medium",
            "evidence": "directive aligns with declared goals; safety inspector clear.",
            "non_goal_check": "no listed non-goals affected",
            "destructive_check": "non-destructive",
            "requires_human": false
        }"#
    }

    #[test]
    fn auto_approve_mode_default_off_when_field_absent() {
        let mode = parse_llm_auto_approve_proposal_mode(&json!({})).expect("default off");
        assert_eq!(mode, LlmAutoApproveProposalMode::Off);
        assert!(!llm_auto_approve_proposal_mode_was_explicit(&json!({})));
    }

    #[test]
    fn auto_approve_mode_recognises_off_blank_and_hyphen() {
        for raw in [
            json!({"auto_approve_mode": "off"}),
            json!({"auto_approve_mode": "  Off  "}),
            json!({"auto_approve_mode": ""}),
        ] {
            let mode = parse_llm_auto_approve_proposal_mode(&raw).expect("off variant");
            assert_eq!(mode, LlmAutoApproveProposalMode::Off);
            assert!(llm_auto_approve_proposal_mode_was_explicit(&raw));
        }
        let sonnet =
            parse_llm_auto_approve_proposal_mode(&json!({"auto_approve_mode": "sonnet_suggest"}))
                .expect("sonnet_suggest");
        assert_eq!(sonnet, LlmAutoApproveProposalMode::SonnetSuggest);
        let hyphen = parse_llm_auto_approve_proposal_mode(
            &json!({"auto_approve_mode": "  Sonnet-Suggest  "}),
        )
        .expect("hyphenated sonnet");
        assert_eq!(hyphen, LlmAutoApproveProposalMode::SonnetSuggest);
    }

    #[test]
    fn auto_approve_mode_rejects_unknown_value() {
        let err = parse_llm_auto_approve_proposal_mode(&json!({"auto_approve_mode": "auto"}))
            .expect_err("typo must fail-fast");
        assert!(err.contains("auto_approve_mode"));
    }

    #[test]
    fn auto_approve_mode_rejects_non_string_type() {
        let err = parse_llm_auto_approve_proposal_mode(&json!({"auto_approve_mode": true}))
            .expect_err("non-string must fail-fast");
        assert!(err.contains("must be a string"));
    }

    #[test]
    fn proposal_mode_label_round_trip() {
        assert_eq!(LlmAutoApproveProposalMode::Off.as_str(), "off");
        assert_eq!(
            LlmAutoApproveProposalMode::SonnetSuggest.as_str(),
            "sonnet_suggest"
        );
        assert!(!LlmAutoApproveProposalMode::Off.is_sonnet_suggest());
        assert!(LlmAutoApproveProposalMode::SonnetSuggest.is_sonnet_suggest());
    }

    #[test]
    fn proposal_status_label_round_trip() {
        assert_eq!(LlmAutoApproveProposalStatus::NotInvoked.as_str(), "not_invoked");
        assert_eq!(LlmAutoApproveProposalStatus::Unavailable.as_str(), "llm_unavailable");
        assert_eq!(LlmAutoApproveProposalStatus::Suggested.as_str(), "suggested");
        assert_eq!(
            LlmAutoApproveProposalStatus::DestructiveBlocked.as_str(),
            "destructive_blocked"
        );
        assert_eq!(LlmAutoApproveProposalStatus::NoSuggestion.as_str(), "no_suggestion");
    }

    #[test]
    fn proposal_confidence_label_round_trip_and_parse() {
        assert_eq!(LlmAutoApproveProposalConfidence::Low.as_str(), "low");
        assert_eq!(LlmAutoApproveProposalConfidence::Medium.as_str(), "medium");
        assert_eq!(LlmAutoApproveProposalConfidence::High.as_str(), "high");
        assert_eq!(
            LlmAutoApproveProposalConfidence::parse("HIGH"),
            Some(LlmAutoApproveProposalConfidence::High)
        );
        assert_eq!(
            LlmAutoApproveProposalConfidence::parse("med"),
            Some(LlmAutoApproveProposalConfidence::Medium)
        );
        assert_eq!(LlmAutoApproveProposalConfidence::parse("foo"), None);
    }

    #[test]
    fn parse_well_formed_proposal_returns_proposal_no_warnings() {
        let (p, warnings) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let p = p.expect("well-formed proposal must parse");
        assert_eq!(p.decision, ReviewDecision::Approved);
        assert_eq!(p.confidence, LlmAutoApproveProposalConfidence::Medium);
        assert!(p.evidence.contains("safety inspector clear"));
        assert_eq!(p.non_goal_check, "no listed non-goals affected");
        assert!(warnings.is_empty(), "well-formed proposal must not warn: {:?}", warnings);
    }

    #[test]
    fn parse_proposal_inside_wrapper_object_accepted() {
        let raw = format!(r#"{{"proposal": {}}}"#, well_formed_proposal_response());
        let (p, warnings) = parse_llm_auto_approve_proposal(&raw);
        assert!(p.is_some(), "wrapper-object proposal must parse");
        assert!(warnings.is_empty());
    }

    #[test]
    fn parse_proposal_strips_code_fence() {
        let fenced = format!("```json\n{}\n```", well_formed_proposal_response());
        let (p, _) = parse_llm_auto_approve_proposal(&fenced);
        assert!(p.is_some());
        let unfenced = format!("```\n{}\n```", well_formed_proposal_response());
        let (p, _) = parse_llm_auto_approve_proposal(&unfenced);
        assert!(p.is_some());
    }

    #[test]
    fn parse_proposal_demotes_rejected_to_needs_changes() {
        let raw = r#"{
            "decision": "rejected",
            "confidence": "high",
            "evidence": "model thinks artifact is unsafe",
            "non_goal_check": "n/a",
            "destructive_check": "n/a",
            "requires_human": true
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        let p = p.expect("rejected proposal must demote, not drop");
        assert_eq!(p.decision, ReviewDecision::NeedsChanges, "invariant I1");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("rule:rejection_demoted")),
            "demotion must be logged: {:?}",
            warnings
        );
    }

    #[test]
    fn parse_proposal_drops_when_evidence_empty() {
        let raw = r#"{
            "decision": "approved",
            "confidence": "high",
            "evidence": "   ",
            "non_goal_check": "n/a",
            "destructive_check": "n/a",
            "requires_human": false
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        assert!(p.is_none(), "empty evidence must drop the proposal");
        assert!(warnings.iter().any(|w| w.contains("evidence")));
    }

    #[test]
    fn parse_proposal_drops_when_decision_missing() {
        let raw = r#"{
            "confidence": "high",
            "evidence": "no decision",
            "non_goal_check": "n/a",
            "destructive_check": "n/a",
            "requires_human": true
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        assert!(p.is_none());
        assert!(warnings.iter().any(|w| w.contains("decision")));
    }

    #[test]
    fn parse_proposal_drops_unknown_decision() {
        let raw = r#"{
            "decision": "unsure",
            "confidence": "high",
            "evidence": "model hedged",
            "non_goal_check": "n/a",
            "destructive_check": "n/a",
            "requires_human": true
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        assert!(p.is_none());
        assert!(warnings.iter().any(|w| w.contains("not in")));
    }

    #[test]
    fn parse_proposal_defaults_low_confidence_when_missing() {
        let raw = r#"{
            "decision": "needs_changes",
            "evidence": "some text",
            "non_goal_check": "ok",
            "destructive_check": "ok",
            "requires_human": true
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        let p = p.expect("missing confidence is non-fatal");
        assert_eq!(p.confidence, LlmAutoApproveProposalConfidence::Low);
        assert!(warnings.iter().any(|w| w.contains("confidence")));
    }

    #[test]
    fn parse_proposal_handles_non_object_top_level() {
        let (p, warnings) = parse_llm_auto_approve_proposal("[1, 2]");
        assert!(p.is_none());
        assert!(warnings
            .iter()
            .any(|w| w.contains("top-level must be an object")));
    }

    #[test]
    fn parse_proposal_handles_invalid_json() {
        let (p, warnings) = parse_llm_auto_approve_proposal("not json at all");
        assert!(p.is_none());
        assert!(warnings.iter().any(|w| w.contains("not valid JSON")));
    }

    #[test]
    fn enforce_invariants_pins_destructive_check_on_archive() {
        let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let mut p = p.take().expect("seed proposal");
        // Sonnet claimed `requires_human=false` and `destructive_check=
        // non-destructive` — the enforcer MUST overwrite.
        let was_destructive = enforce_proposal_invariants(&mut p, "archive");
        assert!(was_destructive, "archive is destructive (invariant I5)");
        assert!(
            p.destructive_check.starts_with("destructive:"),
            "destructive_check must reflect deterministic verdict: {}",
            p.destructive_check
        );
        assert!(
            p.requires_human,
            "invariant I2: destructive actions ALWAYS require human"
        );
    }

    #[test]
    fn enforce_invariants_pins_requires_human_even_on_non_destructive() {
        let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let mut p = p.take().expect("seed proposal");
        // Approve is non-destructive; the model said requires_human=false.
        // Invariant I3 (propose-only) STILL forces requires_human=true.
        let was_destructive = enforce_proposal_invariants(&mut p, "approve");
        assert!(!was_destructive);
        assert!(
            p.destructive_check.starts_with("non_destructive:"),
            "non-destructive verdict must surface: {}",
            p.destructive_check
        );
        assert!(
            p.requires_human,
            "invariant I3: v0 NEVER auto-applies; requires_human always true"
        );
    }

    #[test]
    fn enforce_invariants_recognises_all_destructive_actions() {
        for action in ["archive", "supersede", "remove", "ARCHIVE", "  Supersede  "] {
            let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
            let mut p = p.take().unwrap();
            assert!(
                enforce_proposal_invariants(&mut p, action),
                "`{}` must be destructive",
                action
            );
        }
        for action in ["approve", "compile", "mark", "Approve"] {
            let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
            let mut p = p.take().unwrap();
            assert!(
                !enforce_proposal_invariants(&mut p, action),
                "`{}` must NOT be destructive",
                action
            );
        }
    }

    #[test]
    fn proposal_to_json_pins_applied_false() {
        let (p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let p = p.unwrap();
        let v = p.to_json();
        assert_eq!(
            v.get("applied").and_then(|x| x.as_bool()),
            Some(false),
            "invariant I3: every proposal serialises applied=false"
        );
    }

    #[test]
    fn bundle_not_invoked_records_action_label() {
        let b = LlmAutoApproveProposalBundle::not_invoked("approve");
        assert_eq!(b.mode, LlmAutoApproveProposalMode::Off);
        assert_eq!(b.status, LlmAutoApproveProposalStatus::NotInvoked);
        assert_eq!(b.action, "approve");
        assert!(b.proposal.is_none());
    }

    #[test]
    fn bundle_unavailable_pins_reason_and_caller() {
        let b = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "directive_review_proposer",
            "Sonnet gateway not initialized",
        );
        assert_eq!(b.status, LlmAutoApproveProposalStatus::Unavailable);
        assert!(b
            .unavailable_reason
            .as_deref()
            .unwrap()
            .contains("Sonnet"));
        assert_eq!(b.request_caller.as_deref(), Some("directive_review_proposer"));
        assert!(b.proposal.is_none(), "invariant I4: no fallback proposal");
        assert!(b.proposal_warnings.is_empty());
    }

    #[test]
    fn bundle_destructive_blocked_overwrites_requires_human() {
        let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        // Force model-side claim that no human is needed.
        let proposal = p.take().map(|mut x| {
            x.requires_human = false;
            x
        });
        let b = LlmAutoApproveProposalBundle::destructive_blocked(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "supersede",
            "plan_review_proposer",
            proposal,
            "rule:destructive_action:supersede; auto-approve proposal NEVER promotes destructive actions",
        );
        assert_eq!(b.status, LlmAutoApproveProposalStatus::DestructiveBlocked);
        let p = b.proposal.expect("destructive_blocked preserves proposal");
        assert!(
            p.requires_human,
            "invariant I2: destructive_blocked MUST pin requires_human=true"
        );
        assert!(b
            .proposal_warnings
            .iter()
            .any(|w| w.contains("destructive_action")));
    }

    #[test]
    fn stamp_proposal_payload_round_trip() {
        let (proposal, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let mut proposal = proposal.unwrap();
        enforce_proposal_invariants(&mut proposal, "approve");
        let bundle = LlmAutoApproveProposalBundle {
            mode: LlmAutoApproveProposalMode::SonnetSuggest,
            status: LlmAutoApproveProposalStatus::Suggested,
            proposal: Some(proposal),
            proposal_warnings: vec!["w1".to_string()],
            unavailable_reason: None,
            action: "approve".to_string(),
            request_caller: Some("directive_review_proposer".to_string()),
            model: Some("claude-sonnet".to_string()),
        };
        let mut payload = json!({});
        stamp_llm_auto_approve_proposal_payload(&mut payload, &bundle);
        assert_eq!(payload["llm_auto_approve_proposal_mode"], "sonnet_suggest");
        assert_eq!(payload["llm_auto_approve_proposal_status"], "suggested");
        assert_eq!(payload["llm_auto_approve_proposal_action"], "approve");
        assert_eq!(
            payload["llm_auto_approve_proposal_caller"],
            "directive_review_proposer"
        );
        assert_eq!(payload["llm_auto_approve_proposal_model"], "claude-sonnet");
        assert_eq!(
            payload["llm_auto_approve_proposal_warnings"],
            json!(["w1"])
        );
        assert_eq!(
            payload["llm_auto_approve_proposal"]["applied"],
            false,
            "invariant I3: applied always false"
        );
        assert_eq!(
            payload["llm_auto_approve_proposal"]["requires_human"],
            true,
            "invariant I3: requires_human always true in v0"
        );
        assert_eq!(
            payload["llm_auto_approve_proposal"]["decision"],
            "approved",
            "decision echoed verbatim"
        );
        assert!(payload
            .get("llm_auto_approve_proposal_unavailable_reason")
            .is_none());
    }

    #[test]
    fn stamp_proposal_payload_unavailable_includes_reason() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "directive_review_proposer",
            "no gateway",
        );
        let mut payload = json!({});
        stamp_llm_auto_approve_proposal_payload(&mut payload, &bundle);
        assert_eq!(
            payload["llm_auto_approve_proposal_status"],
            "llm_unavailable"
        );
        assert_eq!(
            payload["llm_auto_approve_proposal_unavailable_reason"],
            "no gateway"
        );
        assert!(
            payload.get("llm_auto_approve_proposal").is_none(),
            "invariant I4: no fallback proposal payload"
        );
    }

    #[test]
    fn proposal_invariants_round_trip_never_surface_rejected() {
        // Defensive invariant I1 — even if a future parser change accepted
        // `rejected`, the stamped payload MUST NOT carry decision=rejected.
        for decision_str in ["approved", "needs_changes", "rejected"] {
            let raw = format!(
                r#"{{
                    "decision": "{}",
                    "confidence": "high",
                    "evidence": "test",
                    "non_goal_check": "n/a",
                    "destructive_check": "n/a",
                    "requires_human": true
                }}"#,
                decision_str
            );
            let (p, _) = parse_llm_auto_approve_proposal(&raw);
            if let Some(mut p) = p {
                enforce_proposal_invariants(&mut p, "approve");
                let v = p.to_json();
                assert_ne!(
                    v["decision"], "rejected",
                    "invariant I1 round-trip: payload MUST NOT carry rejected"
                );
            }
        }
    }

    #[test]
    fn build_proposal_prompts_pure_no_io() {
        let system = build_llm_auto_approve_proposal_system_prompt();
        assert!(system.contains("decision"));
        assert!(system.contains("approved"));
        assert!(system.contains("needs_changes"));
        assert!(system.contains("rejected"));
        assert!(system.contains("requires_human"));
        let user = build_llm_auto_approve_proposal_user_prompt(
            "directive",
            "approve",
            "abc-123",
            1,
            &json!({"deterministic_status": "auto_approved"}),
            Some("(directive :goal :ship)"),
        );
        assert!(user.contains("directive"));
        assert!(user.contains("approve"));
        assert!(user.contains("abc-123"));
        assert!(user.contains("v1"));
        assert!(user.contains("auto_approved"));
        assert!(user.contains("(directive :goal :ship)"));
    }

    // ── wave-22 / task 03 — apply gate v1 unit tests ───────────────────
    //
    // Exercises every code path through `evaluate_llm_approve_apply_gate`
    // PLUS the strict pre-flight `enforce_apply_gate_preflight` PLUS the
    // pure `compute_proposal_hash` helper. Pinned tests for each of the
    // 5 wave-21 / task 06 invariants prove the apply gate cannot break
    // them.

    fn well_formed_high_confidence_proposal() -> LlmAutoApproveProposal {
        LlmAutoApproveProposal {
            decision: ReviewDecision::Approved,
            confidence: LlmAutoApproveProposalConfidence::High,
            evidence: "directive aligns with declared goal; non-goals respected".to_string(),
            non_goal_check: "no scope creep".to_string(),
            destructive_check: "non_destructive:`approve` is not on the destructive list"
                .to_string(),
            requires_human: true,
        }
    }

    fn suggested_bundle(p: LlmAutoApproveProposal) -> LlmAutoApproveProposalBundle {
        LlmAutoApproveProposalBundle {
            mode: LlmAutoApproveProposalMode::SonnetSuggest,
            status: LlmAutoApproveProposalStatus::Suggested,
            proposal: Some(p),
            proposal_warnings: Vec::new(),
            unavailable_reason: None,
            action: "approve".to_string(),
            request_caller: Some("directive_review_proposer".to_string()),
            model: Some("claude-sonnet".to_string()),
        }
    }

    #[test]
    fn apply_gate_compute_proposal_hash_is_deterministic() {
        let p = well_formed_high_confidence_proposal();
        let a = compute_proposal_hash("approve", "abc-123", 1, &p);
        let b = compute_proposal_hash("approve", "abc-123", 1, &p);
        assert_eq!(a, b, "hash MUST be deterministic for identical inputs");
        assert_eq!(a.len(), 32, "hash MUST be exactly 32 hex chars");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "hash MUST be hex");
    }

    #[test]
    fn apply_gate_compute_proposal_hash_changes_on_action() {
        let p = well_formed_high_confidence_proposal();
        let a = compute_proposal_hash("approve", "abc-123", 1, &p);
        let b = compute_proposal_hash("archive", "abc-123", 1, &p);
        assert_ne!(a, b);
    }

    #[test]
    fn apply_gate_compute_proposal_hash_changes_on_decision() {
        let mut p = well_formed_high_confidence_proposal();
        let a = compute_proposal_hash("approve", "abc-123", 1, &p);
        p.decision = ReviewDecision::NeedsChanges;
        let b = compute_proposal_hash("approve", "abc-123", 1, &p);
        assert_ne!(a, b);
    }

    #[test]
    fn apply_gate_compute_proposal_hash_ignores_evidence() {
        let mut p = well_formed_high_confidence_proposal();
        let a = compute_proposal_hash("approve", "abc-123", 1, &p);
        // Free-text fields are intentionally OUT of the hash so superficial
        // wording differences don't churn the audit correlator.
        p.evidence = "completely different wording".to_string();
        p.non_goal_check = "different placeholder".to_string();
        let b = compute_proposal_hash("approve", "abc-123", 1, &p);
        assert_eq!(a, b);
    }

    #[test]
    fn apply_gate_parse_input_default_is_off() {
        let input = parse_llm_approve_apply_gate_input(&json!({})).expect("default ok");
        assert!(!input.apply);
        assert!(!input.caller_approved);
        assert!(input.proposal_hash.is_none());
        assert!(!input.explicit);
    }

    #[test]
    fn apply_gate_parse_input_accepts_full_opt_in() {
        let args = json!({
            "apply_llm_auto_approve": true,
            "proposal_hash": "deadbeef".repeat(4),
            "caller_approved": true,
        });
        let input = parse_llm_approve_apply_gate_input(&args).expect("full opt-in ok");
        assert!(input.apply);
        assert!(input.caller_approved);
        assert_eq!(input.proposal_hash.as_deref(), Some("deadbeef".repeat(4).as_str()));
        assert!(input.explicit);
    }

    #[test]
    fn apply_gate_parse_input_rejects_string_apply() {
        // Strict: literal string `"true"` MUST be rejected so a typo can
        // never silently flip the gate.
        let args = json!({"apply_llm_auto_approve": "true"});
        let err = parse_llm_approve_apply_gate_input(&args).unwrap_err();
        assert_eq!(err.0, APPLY_GATE_INVALID_PARAM);
        assert!(err.1.contains("apply_llm_auto_approve"));
    }

    #[test]
    fn apply_gate_parse_input_rejects_bool_proposal_hash() {
        let args = json!({"proposal_hash": true});
        let err = parse_llm_approve_apply_gate_input(&args).unwrap_err();
        assert_eq!(err.0, APPLY_GATE_INVALID_PARAM);
        assert!(err.1.contains("proposal_hash"));
    }

    #[test]
    fn apply_gate_parse_input_rejects_string_caller_approved() {
        let args = json!({"caller_approved": "yes"});
        let err = parse_llm_approve_apply_gate_input(&args).unwrap_err();
        assert_eq!(err.0, APPLY_GATE_INVALID_PARAM);
    }

    #[test]
    fn apply_gate_parse_input_treats_null_as_absent() {
        let args = json!({
            "apply_llm_auto_approve": null,
            "proposal_hash": null,
            "caller_approved": null,
        });
        let input = parse_llm_approve_apply_gate_input(&args).expect("null = absent");
        assert!(!input.apply);
        assert!(!input.caller_approved);
        assert!(input.proposal_hash.is_none());
        // Explicit because the keys WERE present (even if null).
        assert!(input.explicit);
    }

    #[test]
    fn apply_gate_default_off_returns_not_requested() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let input = LlmApproveApplyGateInput::default();
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::NotRequested);
        assert!(!outcome.status.should_apply());
        assert!(outcome.safety_rule_results.is_empty());
    }

    #[test]
    fn apply_gate_all_six_gates_pass_applies() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash.clone()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::Applied);
        assert!(outcome.status.should_apply());
        assert_eq!(outcome.applied_decision, Some(ReviewDecision::Approved));
        assert_eq!(outcome.proposal_hash_status, ProposalHashStatus::Matches);
        assert!(outcome.caller_approved);
        // Every rule narrated.
        let joined = outcome.safety_rule_results.join("|");
        assert!(joined.contains("rule:non_destructive_action"));
        assert!(joined.contains("rule:proposal_hash:matches"));
        assert!(joined.contains("rule:caller_approved:true"));
        assert!(joined.contains("rule:bundle_status:suggested"));
        assert!(joined.contains("rule:invariant_i5"));
        assert!(joined.contains("rule:decision_approved"));
        assert!(joined.contains("rule:confidence_high"));
        assert!(joined.contains("rule:apply_gate_satisfied"));
    }

    #[test]
    fn apply_gate_invariant_i1_never_applies_needs_changes() {
        // The proposal carries decision=NeedsChanges (the only non-Approved
        // wire value the parser emits — `rejected` is collapsed to
        // NeedsChanges). The gate MUST refuse to apply.
        let mut p = well_formed_high_confidence_proposal();
        p.decision = ReviewDecision::NeedsChanges;
        let bundle = suggested_bundle(p);
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedNonApprovedDecision);
        assert!(!outcome.status.should_apply(), "I1: never auto-anything-non-approve");
        assert_eq!(outcome.applied_decision, Some(ReviewDecision::NeedsChanges));
    }

    #[test]
    fn apply_gate_invariant_i2_destructive_archive_skipped() {
        // Even with a perfect proposal + matching hash + caller_approved,
        // a destructive action MUST skip.
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "archive",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "archive", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedDestructiveAction);
        assert!(!outcome.status.should_apply(), "I2: destructive never auto-promoted");
    }

    #[test]
    fn apply_gate_invariant_i2_destructive_supersede_skipped() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "supersede",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "supersede", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedDestructiveAction);
    }

    #[test]
    fn apply_gate_invariant_i2_destructive_remove_skipped() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "remove",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "remove", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedDestructiveAction);
    }

    #[test]
    fn apply_gate_invariant_i3_proposal_block_unaffected() {
        // The proposal block itself MUST still carry applied=false +
        // requires_human=true regardless of the apply gate's outcome.
        // This is the structural separation the wave-22 / task 03 design
        // depends on.
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let json = bundle.proposal.as_ref().unwrap().to_json();
        assert_eq!(json["applied"], false);
        assert_eq!(json["requires_human"], true);
        // Even after the gate runs and applies, the proposal JSON
        // serialisation is unchanged (the gate publishes its own block).
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::Applied);
        let json2 = bundle.proposal.as_ref().unwrap().to_json();
        assert_eq!(json2["applied"], false, "proposal JSON unchanged by gate");
        assert_eq!(json2["requires_human"], true);
    }

    #[test]
    fn apply_gate_invariant_i4_unavailable_skipped_no_fallback() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "directive_review_proposer",
            "Sonnet gateway not initialized",
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("anything".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedUnavailable);
        assert!(!outcome.status.should_apply(), "I4: never fall back");
        assert_eq!(
            outcome.proposal_hash_status,
            ProposalHashStatus::NoProposalAvailable
        );
    }

    #[test]
    fn apply_gate_invariant_i5_destructive_check_always_deterministic() {
        // Construct a proposal whose model-supplied destructive_check
        // says "non_destructive" but the deterministic action label is
        // "archive" (destructive). The gate MUST trust the deterministic
        // verdict, NEVER the model.
        let mut p = well_formed_high_confidence_proposal();
        p.destructive_check = "non_destructive:model_lied_here".to_string();
        let bundle = suggested_bundle(p);
        let hash = compute_proposal_hash(
            "archive",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "archive", "abc-123", 1);
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::SkippedDestructiveAction,
            "I5: deterministic destructive verdict overrides model claim"
        );
    }

    #[test]
    fn apply_gate_skips_when_caller_approved_false() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: false,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedCallerNotApproved);
    }

    #[test]
    fn apply_gate_skips_when_confidence_medium() {
        let mut p = well_formed_high_confidence_proposal();
        p.confidence = LlmAutoApproveProposalConfidence::Medium;
        let bundle = suggested_bundle(p);
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedConfidenceTooLow);
    }

    #[test]
    fn apply_gate_skips_when_confidence_low() {
        let mut p = well_formed_high_confidence_proposal();
        p.confidence = LlmAutoApproveProposalConfidence::Low;
        let bundle = suggested_bundle(p);
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedConfidenceTooLow);
    }

    #[test]
    fn apply_gate_skips_when_bundle_is_no_suggestion() {
        let bundle = LlmAutoApproveProposalBundle {
            mode: LlmAutoApproveProposalMode::SonnetSuggest,
            status: LlmAutoApproveProposalStatus::NoSuggestion,
            proposal: None,
            proposal_warnings: vec!["unparseable response".to_string()],
            unavailable_reason: None,
            action: "approve".to_string(),
            request_caller: Some("directive_review_proposer".to_string()),
            model: Some("claude-sonnet".to_string()),
        };
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("any".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedNoProposal);
    }

    #[test]
    fn apply_gate_skips_when_bundle_is_not_invoked() {
        let bundle = LlmAutoApproveProposalBundle::not_invoked("approve");
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("any".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedNoProposal);
    }

    #[test]
    fn apply_gate_preflight_requires_proposal_hash_under_apply_true() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: None,
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1)
            .unwrap_err();
        assert_eq!(err.0, APPLY_GATE_MISSING_PROPOSAL_HASH);
        assert!(err.1.contains("proposal_hash"));
    }

    #[test]
    fn apply_gate_preflight_rejects_mismatched_hash() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("0".repeat(32)),
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1)
            .unwrap_err();
        assert_eq!(err.0, APPLY_GATE_PROPOSAL_HASH_MISMATCH);
        assert!(err.1.contains("does not match"));
    }

    #[test]
    fn apply_gate_preflight_accepts_matching_hash_case_insensitive() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash.to_uppercase()),
            caller_approved: true,
            explicit: true,
        };
        assert!(
            enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1).is_ok(),
            "preflight MUST accept case-insensitive hash match"
        );
    }

    #[test]
    fn apply_gate_preflight_skips_when_apply_false() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let input = LlmApproveApplyGateInput {
            apply: false,
            proposal_hash: Some("garbage".to_string()),
            caller_approved: false,
            explicit: true,
        };
        // apply=false ⇒ preflight passes without checking the hash.
        assert!(enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1).is_ok());
    }

    #[test]
    fn apply_gate_preflight_no_proposal_with_hash_returns_mismatch() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "x",
            "down",
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("anything".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1)
            .unwrap_err();
        assert_eq!(err.0, APPLY_GATE_PROPOSAL_HASH_MISMATCH);
    }

    #[test]
    fn apply_gate_preflight_no_proposal_no_hash_returns_missing() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "x",
            "down",
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: None,
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1)
            .unwrap_err();
        assert_eq!(err.0, APPLY_GATE_MISSING_PROPOSAL_HASH);
    }

    #[test]
    fn apply_gate_stamp_payload_emits_full_block_when_applied() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash.clone()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        let mut payload = json!({"status": "approved"});
        stamp_llm_approve_apply_gate_payload(&mut payload, &outcome);
        let block = payload
            .get("llm_approve_apply_gate")
            .expect("gate block stamped");
        assert_eq!(block["apply_status"], "applied");
        assert_eq!(block["applied_decision"], "approved");
        assert_eq!(block["proposal_hash_status"], "matches");
        assert_eq!(block["caller_approved"], true);
        assert_eq!(block["computed_proposal_hash"], hash);
        assert_eq!(block["supplied_proposal_hash"], hash);
        assert!(
            block["safety_rule_results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str().unwrap_or("").contains("apply_gate_satisfied")),
            "apply_gate_satisfied rule MUST surface"
        );
    }

    #[test]
    fn apply_gate_stamp_payload_omits_block_when_not_requested() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let outcome = evaluate_llm_approve_apply_gate(
            &LlmApproveApplyGateInput::default(),
            &bundle,
            "approve",
            "abc-123",
            1,
        );
        let mut payload = json!({"status": "approved"});
        stamp_llm_approve_apply_gate_payload(&mut payload, &outcome);
        assert!(
            payload.get("llm_approve_apply_gate").is_none(),
            "default off MUST stay byte-identical with wave-21 / task 06"
        );
    }

    #[test]
    fn apply_gate_stamp_proposal_hash_payload_when_present() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let mut payload = json!({});
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", "abc-123", 1);
        let hash = payload
            .get("llm_auto_approve_proposal_hash")
            .and_then(|v| v.as_str())
            .expect("hash stamped");
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn apply_gate_stamp_proposal_hash_payload_skips_when_absent() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "x",
            "down",
        );
        let mut payload = json!({});
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", "abc-123", 1);
        assert!(payload.get("llm_auto_approve_proposal_hash").is_none());
    }

    // ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
    //
    // Pin the wave22-03 review LLM auto-approve apply gate slice of the
    // wave22-07 v4 smoke contract. The pure evaluator + preflight pair
    // is the deterministic SSOT — no Sonnet call, no DB transition,
    // pure in-process functions over synthesised proposal/bundle structs.
    // The companion plan.rs / workstation_dispatch.rs / agent_execution.rs
    // / unified_entry.rs smokes cover the persisted-apply, auto-spawn,
    // failed-verification and markdown-non-load-bearing slices.

    /// V4 smoke (Requirement 2 / review apply-gate slice): the apply
    /// gate MUST reject `apply_llm_auto_approve=true` when the caller
    /// does not supply `proposal_hash`, AND MUST accept the same call
    /// when a hash matching `compute_proposal_hash(action, artifact_id,
    /// version, proposal)` is supplied. This is the wave22-03 gate's
    /// fail-fast preflight — the gate refuses to mutate state with no
    /// correlator and accepts only the canonical fixture path.
    #[test]
    fn smoke_wave22_07_review_apply_gate_rejects_missing_hash_accepts_fixture_hash() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        // Missing proposal_hash → APPLY_GATE_MISSING_PROPOSAL_HASH.
        let missing_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: None,
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&missing_input, &bundle, "approve", "abc-123", 1)
            .expect_err("wave22-07 v4: missing proposal_hash MUST fail-fast");
        assert_eq!(
            err.0, APPLY_GATE_MISSING_PROPOSAL_HASH,
            "wave22-07 v4 invariant: missing proposal_hash MUST surface the dedicated code"
        );
        // Mismatched proposal_hash → APPLY_GATE_PROPOSAL_HASH_MISMATCH.
        let mismatch_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("0".repeat(32)),
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&mismatch_input, &bundle, "approve", "abc-123", 1)
            .expect_err("wave22-07 v4: mismatched proposal_hash MUST fail-fast");
        assert_eq!(err.0, APPLY_GATE_PROPOSAL_HASH_MISMATCH);
        // Matching fixture hash → preflight OK + gate Applied.
        let canonical = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let valid_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(canonical.clone()),
            caller_approved: true,
            explicit: true,
        };
        assert!(
            enforce_apply_gate_preflight(&valid_input, &bundle, "approve", "abc-123", 1).is_ok(),
            "wave22-07 v4: matching proposal_hash MUST pass preflight"
        );
        let outcome = evaluate_llm_approve_apply_gate(
            &valid_input,
            &bundle,
            "approve",
            "abc-123",
            1,
        );
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::Applied,
            "wave22-07 v4 invariant: matching fixture proposal_hash MUST drive the gate to Applied"
        );
        assert_eq!(outcome.proposal_hash_status, ProposalHashStatus::Matches);
    }

    /// V4 smoke (cross-wave invariants / wave21-06 5 invariants pinned):
    /// the wave22-03 apply gate MUST preserve every wave-21 / task 06
    /// propose-only invariant when stamped onto the same call.
    ///   * I1 never auto-reject — the gate MUST refuse to apply a
    ///     `decision=NeedsChanges` proposal.
    ///   * I2 destructive never promote — destructive actions
    ///     (archive / supersede / remove) MUST skip even when every
    ///     other gate is green.
    ///   * I3 proposal block applied=false / requires_human=true —
    ///     the propose-only proposal serialisation MUST stay
    ///     unchanged even after the apply gate has run.
    ///   * I4 Sonnet unavailable no fallback — the gate MUST short-
    ///     circuit on `Unavailable` bundles without falling back.
    ///   * I5 destructive_check ALWAYS deterministic — a model that
    ///     claimed `non_destructive` MUST be overridden by the
    ///     deterministic destructive verdict.
    #[test]
    fn smoke_wave22_07_review_apply_gate_pins_wave21_06_five_invariants() {
        // I1 — never auto-reject (NeedsChanges).
        let mut needs_changes = well_formed_high_confidence_proposal();
        needs_changes.decision = ReviewDecision::NeedsChanges;
        let nc_bundle = suggested_bundle(needs_changes);
        let nc_hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            nc_bundle.proposal.as_ref().unwrap(),
        );
        let nc_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(nc_hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome =
            evaluate_llm_approve_apply_gate(&nc_input, &nc_bundle, "approve", "abc-123", 1);
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::SkippedNonApprovedDecision,
            "wave21-06 I1: never auto-anything-non-approve"
        );
        // I2 — destructive never promote (archive / supersede / remove).
        for destructive in ["archive", "supersede", "remove"] {
            let bundle = suggested_bundle(well_formed_high_confidence_proposal());
            let hash = compute_proposal_hash(
                destructive,
                "abc-123",
                1,
                bundle.proposal.as_ref().unwrap(),
            );
            let input = LlmApproveApplyGateInput {
                apply: true,
                proposal_hash: Some(hash),
                caller_approved: true,
                explicit: true,
            };
            let outcome = evaluate_llm_approve_apply_gate(
                &input,
                &bundle,
                destructive,
                "abc-123",
                1,
            );
            assert_eq!(
                outcome.status,
                LlmApproveApplyStatus::SkippedDestructiveAction,
                "wave21-06 I2: destructive `{}` MUST never auto-promote",
                destructive
            );
        }
        // I3 — proposal block applied=false / requires_human=true even
        //      after the apply gate has driven Applied.
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::Applied);
        let proposal_json = bundle.proposal.as_ref().unwrap().to_json();
        assert_eq!(
            proposal_json["applied"], false,
            "wave21-06 I3: proposal serialisation MUST keep applied=false even when gate Applied"
        );
        assert_eq!(
            proposal_json["requires_human"], true,
            "wave21-06 I3: proposal serialisation MUST keep requires_human=true"
        );
        // I4 — Sonnet unavailable no fallback.
        let unavailable = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "wave22-07-v4-smoke",
            "Sonnet gateway not initialized",
        );
        let unavail_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("anything".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(
            &unavail_input,
            &unavailable,
            "approve",
            "abc-123",
            1,
        );
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::SkippedUnavailable,
            "wave21-06 I4: Sonnet unavailable MUST never fall back"
        );
        // I5 — destructive_check ALWAYS deterministic; a model-supplied
        //      `non_destructive:` string is overridden by the deterministic
        //      destructive verdict for `archive`.
        let mut model_lied = well_formed_high_confidence_proposal();
        model_lied.destructive_check = "non_destructive:model_lied_here".to_string();
        let lied_bundle = suggested_bundle(model_lied);
        let lied_hash = compute_proposal_hash(
            "archive",
            "abc-123",
            1,
            lied_bundle.proposal.as_ref().unwrap(),
        );
        let lied_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(lied_hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(
            &lied_input,
            &lied_bundle,
            "archive",
            "abc-123",
            1,
        );
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::SkippedDestructiveAction,
            "wave21-06 I5: deterministic destructive verdict overrides model claim"
        );
    }
}
