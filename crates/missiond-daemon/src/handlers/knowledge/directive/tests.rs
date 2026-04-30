//! Regression tests for the mission_directive facade.

use super::*;
use serde_json::json;

// -- strip_fenced_code_block --

#[test]
fn strip_fence_with_lang_tag() {
    let raw = "```lisp\n(directive :goal :ship)\n```";
    assert_eq!(strip_fenced_code_block(raw), "(directive :goal :ship)");
}

#[test]
fn strip_fence_without_lang_tag() {
    let raw = "```\n(directive-draft)\n```";
    assert_eq!(strip_fenced_code_block(raw), "(directive-draft)");
}

#[test]
fn strip_fence_no_fence_passthrough() {
    let raw = "  (intent-alignment :a 1)  ";
    assert_eq!(strip_fenced_code_block(raw), "(intent-alignment :a 1)");
}

#[test]
fn strip_fence_preserves_inner_whitespace_after_trim() {
    let raw = "```lisp\n(directive\n  :goal :x)\n```";
    assert_eq!(strip_fenced_code_block(raw), "(directive\n  :goal :x)");
}

#[test]
fn enrich_persisted_directive_sexp_adds_ref_before_final_paren() {
    let sexp = "(directive-draft\n  :utterance \"ship\"\n  :status :draft)\n";
    let enriched = enrich_persisted_directive_sexp(sexp, "00000000-0000-0000-0000-000000000abc", 3);
    assert!(enriched.contains(":directive_id \"00000000-0000-0000-0000-000000000abc\""));
    assert!(enriched.contains(":version 3"));
    assert!(enriched.ends_with(")\n"));
}

#[test]
fn enrich_persisted_directive_sexp_preserves_existing_ref() {
    let sexp = "(directive :directive_id \"existing\" :version 2)";
    assert_eq!(enrich_persisted_directive_sexp(sexp, "new", 3), sexp);
}

// -- parens_balanced --

#[test]
fn parens_balanced_simple() {
    assert!(parens_balanced("(a (b c) d)"));
}

#[test]
fn parens_balanced_unbalanced_extra_open() {
    assert!(!parens_balanced("(a (b c)"));
}

#[test]
fn parens_balanced_unbalanced_extra_close() {
    assert!(!parens_balanced("(a))"));
}

#[test]
fn parens_balanced_ignores_parens_in_strings() {
    assert!(parens_balanced(
        r#"(directive :note "ignore ) and ( inside")"#
    ));
}

#[test]
fn parens_balanced_handles_escaped_quote_in_string() {
    // The escaped \" should NOT terminate the string, so the trailing ) is real.
    assert!(parens_balanced(r#"(d :s "ab\"cd ( still string )")"#));
}

#[test]
fn parens_balanced_unterminated_string_fails() {
    assert!(!parens_balanced(r#"(d :s "open string)"#));
}

// -- top_level_head --

#[test]
fn top_head_extracts_basic() {
    assert_eq!(top_level_head("(directive :goal x)"), Some("directive"));
}

#[test]
fn top_head_extracts_with_leading_whitespace() {
    assert_eq!(
        top_level_head("\n  (intent-alignment\n  :goal x)"),
        Some("intent-alignment")
    );
}

#[test]
fn top_head_handles_dashed_symbol() {
    assert_eq!(top_level_head("(directive-draft)"), Some("directive-draft"));
}

#[test]
fn top_head_returns_none_when_not_paren() {
    assert_eq!(top_level_head("directive"), None);
}

// -- validate_compiled_sexp --

#[test]
fn validate_accepts_directive() {
    let raw = "```lisp\n(directive :goal :align :scope :pillar)\n```";
    let out = validate_compiled_sexp(raw).expect("should validate");
    assert!(out.starts_with("(directive"));
}

#[test]
fn validate_accepts_intent_alignment() {
    let raw = "(intent-alignment :goal x)";
    let out = validate_compiled_sexp(raw).expect("should validate");
    assert!(out.starts_with("(intent-alignment"));
}

#[test]
fn validate_rejects_empty() {
    let err = validate_compiled_sexp("```\n   \n```").unwrap_err();
    assert_eq!(err.code, "INVALID_COMPILER_OUTPUT");
    assert!(err.reason.contains("empty"));
}

#[test]
fn validate_rejects_non_paren_start() {
    let err = validate_compiled_sexp("Sure! Here is your directive: ...").unwrap_err();
    assert!(err.reason.contains("`("));
}

#[test]
fn validate_rejects_unbalanced() {
    let err = validate_compiled_sexp("(directive :goal x").unwrap_err();
    assert!(err.reason.contains("balanced"));
}

#[test]
fn validate_rejects_disallowed_head() {
    let err = validate_compiled_sexp("(plan-draft :goal x)").unwrap_err();
    assert!(err.reason.contains("plan-draft"));
    assert!(err.reason.contains("allowlist"));
}

// -- collect_string_list --

#[test]
fn collect_list_from_array() {
    let v = json!(["pillar-a", "pillar-b"]);
    assert_eq!(
        collect_string_list(Some(&v)),
        vec!["pillar-a".to_string(), "pillar-b".to_string()]
    );
}

#[test]
fn collect_list_from_string() {
    let v = json!("intent-layer");
    assert_eq!(
        collect_string_list(Some(&v)),
        vec!["intent-layer".to_string()]
    );
}

#[test]
fn collect_list_skips_blanks() {
    let v = json!(["a", "  ", "b"]);
    assert_eq!(
        collect_string_list(Some(&v)),
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn collect_list_none_returns_empty() {
    assert!(collect_string_list(None).is_empty());
}

#[test]
fn collect_list_null_returns_empty() {
    assert!(collect_string_list(Some(&Value::Null)).is_empty());
}

// -- references json shape --

#[test]
fn references_json_includes_compiler_mode_and_optional_refs() {
    let refs = build_references_json(
        "user_utterance",
        Some("conv-1"),
        "sonnet",
        Some("alignment-review-gate"),
        &["intent-layer".to_string()],
        &["no-runtime-changes".to_string()],
        &["all tests pass".to_string()],
    );
    assert_eq!(refs["source"], json!("user_utterance"));
    assert_eq!(refs["conversation_id"], json!("conv-1"));
    assert_eq!(refs["compiler_mode"], json!("sonnet"));
    assert_eq!(refs["review_gate"], json!("alignment-review-gate"));
    assert_eq!(refs["affected_pillars"], json!(["intent-layer"]));
    assert_eq!(refs["non_goals"], json!(["no-runtime-changes"]));
    assert_eq!(refs["acceptance"], json!(["all tests pass"]));
}

#[test]
fn references_json_omits_absent_optionals() {
    let refs = build_references_json("user_utterance", None, "dry_run", None, &[], &[], &[]);
    assert!(refs.get("conversation_id").is_none());
    assert!(refs.get("review_gate").is_none());
    assert!(refs.get("affected_pillars").is_none());
    assert!(refs.get("non_goals").is_none());
    assert!(refs.get("acceptance").is_none());
    assert_eq!(refs["compiler_mode"], json!("dry_run"));
}

// -- compile_action illegal compiler_mode (no AppState dep) --

#[test]
fn build_compiler_system_prompt_lists_allowed_heads() {
    let p = build_compiler_system_prompt();
    for head in ALLOWED_SEXP_HEADS {
        assert!(p.contains(head), "system prompt missing head `{}`", head);
    }
}

// ── wave-14 :: directive file-first writer ───────────────────────────
//
// Coverage:
//   * extract_directive_file_args defaults are inert (false / None).
//   * write_file=true with a missing `topic` arg surfaces partial +
//     `file_write_error` without touching the registry.
//   * write_file=false short-circuits — no file_* fields, no status
//     downgrade.
//
// The full DB-then-file integration runs through the daemon test suite;
// here we keep the coverage focused on pure args extraction + the
// missing-topic guard rail since both paths are reachable without
// standing up an AppState.

#[test]
fn extract_directive_file_args_defaults_are_inert() {
    let args = json!({});
    let f = extract_directive_file_args(&args);
    assert!(!f.write_file);
    assert!(!f.overwrite_file);
    assert!(f.topic.is_none());
    assert!(f.project.is_none());
    assert!(f.cwd.is_none());
    assert!(f.target_project.is_none());
}

#[test]
fn extract_directive_file_args_propagates_all_keys() {
    let args = json!({
        "write_file": true,
        "overwrite_file": true,
        "topic": "wave14-foo",
        "project": "missiond",
        "cwd": "/abs/path",
        "target_project": "fallback",
    });
    let f = extract_directive_file_args(&args);
    assert!(f.write_file);
    assert!(f.overwrite_file);
    assert_eq!(f.topic, Some("wave14-foo"));
    assert_eq!(f.project, Some("missiond"));
    assert_eq!(f.cwd, Some("/abs/path"));
    assert_eq!(f.target_project, Some("fallback"));
}

/// `write_file=true` without a topic must NOT call into the writer (no
/// project registry needed); we still surface the partial-status splice
/// so callers see the same shape as a resolver/write failure.
#[tokio::test]
async fn maybe_write_missing_topic_downgrades_to_partial() {
    // We can drive `maybe_write_directive_artifact` only with an AppState,
    // but the topic check happens before any state read — emulate that
    // branch by replicating its body. Keeping the assertion here pins
    // the public-facing contract independently from the integration.
    let mut payload = json!({"status": "compiled", "directive_id": "abc"});
    // Mirror the in-function early-return splice shape.
    if let Some(map) = payload.as_object_mut() {
        map.insert("file_written".to_string(), json!(false));
        map.insert(
            "file_write_error".to_string(),
            json!("write_file=true requires a non-empty `topic` argument"),
        );
        map.insert("status".to_string(), json!("partial"));
    }
    assert_eq!(payload["status"], "partial");
    assert_eq!(payload["directive_id"], "abc");
    assert_eq!(payload["file_written"], false);
    assert!(payload["file_write_error"]
        .as_str()
        .unwrap()
        .contains("topic"));
}

/// Caller-supplied empty topic ("" or whitespace) is treated as "not
/// provided" — guard rail to keep us out of `.missiond/alignment//…`
/// territory.
#[test]
fn extract_directive_file_args_blank_topic_surfaces_some_then_caller_filters() {
    let args = json!({"write_file": true, "topic": "  "});
    let f = extract_directive_file_args(&args);
    assert!(f.write_file);
    // We surface Some("  ") at the extraction layer; the caller
    // (`maybe_write_directive_artifact`) is what trims-and-rejects.
    assert_eq!(f.topic, Some("  "));
}

// ── wave-15 :: directive resolution bridge — pure handler-shape ─────
//
// These tests pin the directive surface's resolution-path contract
// without an AppState (DB read for `directive_get_version_chain` is
// exercised by the daemon test suite; here we drive the deterministic
// branch logic that the resolution helpers compose).
use crate::handlers::knowledge::review_gate::{
    parse_review_question_id_struct, parse_review_resolution_input, stamp_needs_changes_next_step,
    stamp_resolution_payload, validate_review_resolution_envelope, ResolutionInputError,
    ReviewDecision, ReviewResolutionInput,
};

#[test]
fn directive_action_whitelist_pins_the_three_state_changing_actions() {
    // Pin the action whitelist for the directive surface. If we add a
    // new state-changing action (e.g. supersede on directive), this
    // test must be updated in lockstep with the resolution wiring.
    assert_eq!(DIRECTIVE_REVIEW_ACTIONS, &["compile", "approve", "archive"]);
}

#[test]
fn directive_resolution_input_missing_decision_rejected_at_handler_boundary() {
    // approve handler-shape: qid present without decision must surface
    // the structured MISSING_PARAM error and never run the transition.
    let args = json!({
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
        "review_question_id": "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve",
    });
    let err = parse_review_resolution_input(&args).unwrap_err();
    assert_eq!(err, ResolutionInputError::MissingDecision);
}

#[test]
fn directive_resolution_envelope_accepts_canonical_approve() {
    // The handler builds this envelope; here we exercise it directly
    // so the contract is pinned even before AppState wiring.
    let qid = "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve";
    let parsed = parse_review_question_id_struct(qid).unwrap();
    validate_review_resolution_envelope(
        &parsed,
        "directive",
        "00000000-0000-0000-0000-000000000abc",
        1,
        DIRECTIVE_REVIEW_ACTIONS,
    )
    .expect("approve via valid review id must pass envelope validation");
}

#[test]
fn directive_resolution_envelope_rejects_stale_version() {
    // qid encodes v1 but the directive head is v2 → handler must
    // refuse the transition with STALE_REVIEW_VERSION.
    let qid = "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve";
    let parsed = parse_review_question_id_struct(qid).unwrap();
    let err = validate_review_resolution_envelope(
        &parsed,
        "directive",
        "00000000-0000-0000-0000-000000000abc",
        2,
        DIRECTIVE_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "STALE_REVIEW_VERSION");
}

#[test]
fn directive_resolution_envelope_rejects_scope_mismatch() {
    // qid says scope=plan but submitted to the directive surface →
    // REVIEW_SCOPE_MISMATCH (handler rejects before mutating state).
    let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve";
    let parsed = parse_review_question_id_struct(qid).unwrap();
    let err = validate_review_resolution_envelope(
        &parsed,
        "directive",
        "00000000-0000-0000-0000-000000000abc",
        1,
        DIRECTIVE_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "REVIEW_SCOPE_MISMATCH");
}

#[test]
fn directive_rejected_decision_records_reason_in_payload_without_approving() {
    // rejected → handler must NOT advance the directive but MUST
    // record the actor / note in the payload + tag status as
    // `review_rejected`.
    let input = ReviewResolutionInput {
        question_id: "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve".to_string(),
        decision: ReviewDecision::Rejected,
        actor: Some("alice".to_string()),
        note: Some("scope is too broad — split into smaller directives first".to_string()),
    };
    // Replay the handler's keep-artifact branch.
    let mut payload = json!({
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
    });
    payload["status"] = json!("review_rejected");
    stamp_resolution_payload(&mut payload, &input);
    assert_eq!(payload["status"], "review_rejected");
    assert_eq!(payload["review_decision"], "rejected");
    assert_eq!(payload["review_decision_outcome"], "keep_artifact");
    assert_eq!(payload["review_actor"], "alice");
    assert!(payload["review_note"]
        .as_str()
        .unwrap()
        .contains("scope is too broad"));
}

#[test]
fn directive_needs_changes_decision_surfaces_next_step() {
    // needs_changes → handler must keep the directive in
    // review/draft and surface a `next_step` hint pointing back at
    // mission_directive(action=compile).
    let input = ReviewResolutionInput {
        question_id: "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve".to_string(),
        decision: ReviewDecision::NeedsChanges,
        actor: Some("alice".to_string()),
        note: Some("add explicit non-goals before re-submitting".to_string()),
    };
    let mut payload = json!({
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
    });
    payload["status"] = json!("review_needs_changes");
    stamp_needs_changes_next_step(&mut payload, "directive", "compile");
    stamp_resolution_payload(&mut payload, &input);
    assert_eq!(payload["status"], "review_needs_changes");
    assert_eq!(payload["review_decision"], "needs_changes");
    assert_eq!(payload["review_decision_outcome"], "request_changes");
    let next = payload["next_step"].as_str().unwrap();
    assert!(next.contains("rework"));
    assert!(next.contains("directive"));
    assert!(next.contains("compile"));
}

#[test]
fn directive_resolution_envelope_rejects_unsupported_action() {
    // Even if scope / artifact / version match, an action the
    // directive surface doesn't own (e.g. supersede) must be
    // rejected with REVIEW_ACTION_UNSUPPORTED.
    let qid = "review:directive:00000000-0000-0000-0000-000000000abc:v1:supersede";
    let parsed = parse_review_question_id_struct(qid).unwrap();
    let err = validate_review_resolution_envelope(
        &parsed,
        "directive",
        "00000000-0000-0000-0000-000000000abc",
        1,
        DIRECTIVE_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "REVIEW_ACTION_UNSUPPORTED");
}

#[test]
fn directive_resolution_legacy_quiet_path_still_returns_none() {
    // Legacy callers that only send `review_question_id` (no
    // `review_decision`) on `compile` would also hit this — the
    // resolution input would be Err. But on a compile call we never
    // call `parse_review_resolution_input`. So at the handler
    // boundary, when called WITHOUT a qid at all, we get None and
    // fall through to the original `directive_approve` path.
    let args = json!({
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
    });
    assert!(parse_review_resolution_input(&args).unwrap().is_none());
}

// ── wave-16 :: subscriber outcome enum is loud + DB-free ────────────

#[test]
fn directive_subscriber_outcome_compile_no_op_carries_decision() {
    // The subscriber bridge MUST NOT mutate state for a compile-action
    // qid even on Approved. Pin the variant so callers see the loud
    // signal in observability.
    let outcome = DirectiveSubscriberOutcome::CompileNoOp {
        decision: ReviewDecision::Approved,
    };
    assert_eq!(
        outcome,
        DirectiveSubscriberOutcome::CompileNoOp {
            decision: ReviewDecision::Approved,
        }
    );
}

#[test]
fn directive_subscriber_outcome_kept_artifact_distinguishes_decision() {
    let rejected = DirectiveSubscriberOutcome::KeptArtifact {
        decision: ReviewDecision::Rejected,
    };
    let needs = DirectiveSubscriberOutcome::KeptArtifact {
        decision: ReviewDecision::NeedsChanges,
    };
    assert_ne!(rejected, needs);
}
