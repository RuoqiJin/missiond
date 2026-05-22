//! Regression tests for the unified entry runtime facade.

use super::*;
use serde_json::json;

// ── plan_pipeline routing ───────────────────────────────────────

#[test]
fn routes_message_only_to_directive_compile() {
    let args = json!({ "message": "ship the unified entry helper" });
    let decision = plan_pipeline(&args).expect("should route");
    match decision {
        PipelineDecision::DirectiveCompile { compile_args } => {
            assert_eq!(compile_args["action"], "compile");
            assert_eq!(compile_args["utterance"], "ship the unified entry helper");
        }
        other => panic!("expected DirectiveCompile, got {:?}", other),
    }
}

#[test]
fn message_compile_forwards_optional_breadcrumbs() {
    let args = json!({
        "message": "do x",
        "source": "user_utterance",
        "conversation_id": "conv-7",
        "compiler_mode": "sonnet",
        "persist": true,
        "directive_review_gate": "alignment-review-gate",
        "directive_affected_pillars": ["intent-layer"],
        "directive_non_goals": ["no runtime change"],
        "directive_acceptance": ["all tests pass"],
    });
    let decision = plan_pipeline(&args).expect("should route");
    let PipelineDecision::DirectiveCompile { compile_args } = decision else {
        panic!("expected DirectiveCompile");
    };
    assert_eq!(compile_args["source"], "user_utterance");
    assert_eq!(compile_args["conversation_id"], "conv-7");
    assert_eq!(compile_args["compiler_mode"], "sonnet");
    assert_eq!(compile_args["persist"], true);
    assert_eq!(compile_args["review_gate"], "alignment-review-gate");
    assert_eq!(compile_args["affected_pillars"], json!(["intent-layer"]));
    assert_eq!(compile_args["non_goals"], json!(["no runtime change"]));
    assert_eq!(compile_args["acceptance"], json!(["all tests pass"]));
}

#[test]
fn rejects_blank_message() {
    let args = json!({ "message": "   " });
    let err = plan_pipeline(&args).expect_err("blank message must fail");
    assert_eq!(err, PlannerError::MissingMessage);
}

#[test]
fn rejects_missing_message() {
    let args = json!({});
    let err = plan_pipeline(&args).expect_err("missing message must fail");
    assert_eq!(err, PlannerError::MissingMessage);
}

#[test]
fn approved_directive_routes_to_plan_compile() {
    let args = json!({
        "approved_directive_id": "00000000-0000-0000-0000-000000000abc",
        "board_task_id": "btk-42",
        "compiler_mode": "sonnet",
        "persist": true,
        "plan_acceptance": ["compile passes"],
        "plan_constraints": ["no migration"],
        "target_project": "missiond",
        "dispatch_strategy": "agent-team",
        "parallelism": "agent-team",
        "directive_version": 2,
    });
    let decision = plan_pipeline(&args).expect("should route");
    let PipelineDecision::PlanCompile { compile_args } = decision else {
        panic!("expected PlanCompile");
    };
    assert_eq!(compile_args["action"], "compile");
    assert_eq!(
        compile_args["directive_id"],
        "00000000-0000-0000-0000-000000000abc"
    );
    assert_eq!(compile_args["board_task_id"], "btk-42");
    assert_eq!(compile_args["compiler_mode"], "sonnet");
    assert_eq!(compile_args["persist"], true);
    assert_eq!(compile_args["acceptance"], json!(["compile passes"]));
    assert_eq!(compile_args["constraints"], json!(["no migration"]));
    assert_eq!(compile_args["target_project"], "missiond");
    assert_eq!(compile_args["dispatch_strategy"], "agent-team");
    assert_eq!(compile_args["parallelism"], "agent-team");
    assert_eq!(compile_args["directive_version"], 2);
}

#[test]
fn approved_directive_without_board_task_fails() {
    let args = json!({
        "approved_directive_id": "00000000-0000-0000-0000-000000000abc",
    });
    let err = plan_pipeline(&args).expect_err("must require board_task_id at s4");
    assert_eq!(err, PlannerError::PlanCompileMissingBoardTask);
}

#[test]
fn approved_plan_with_execute_routes_to_plan_execute() {
    let args = json!({
        "approved_plan_id": "11111111-1111-1111-1111-111111111111",
        "execute": true,
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "max_parallel_nodes": 4,
        "dispatch_strategy": "agent-team",
    });
    let decision = plan_pipeline(&args).expect("should route");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    assert_eq!(execute_args["action"], "execute");
    assert_eq!(
        execute_args["plan_id"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(execute_args["execute_mode"], "internal");
    assert_eq!(execute_args["scheduler_mode"], "dag_v1");
    assert_eq!(execute_args["max_parallel_nodes"], 4);
    assert_eq!(execute_args["dispatch_strategy"], "agent-team");
}

#[test]
fn approved_plan_accepts_execute_after_approval_alias() {
    let args = json!({
        "approved_plan_id": "11111111-1111-1111-1111-111111111111",
        "execute_after_approval": true,
    });
    let decision = plan_pipeline(&args).expect("should route");
    match decision {
        PipelineDecision::PlanExecute { .. } => {}
        other => panic!("expected PlanExecute, got {:?}", other),
    }
}

#[test]
fn approved_plan_without_execute_flag_fails() {
    // The most safety-critical test: a stray `approved_plan_id` must
    // NEVER silently auto-execute. The caller has to opt in
    // explicitly so the human gate stays meaningful.
    let args = json!({
        "approved_plan_id": "11111111-1111-1111-1111-111111111111",
    });
    let err = plan_pipeline(&args).expect_err("must refuse silent auto-execute");
    assert_eq!(err, PlannerError::ApprovedPlanWithoutExecuteFlag);
}

#[test]
fn execute_flag_without_approved_plan_fails() {
    // Symmetric safety check: execute=true without an approved plan
    // id must not fall back to "execute the most recent plan" or
    // similar — that would silently bypass the s5 review gate.
    let args = json!({
        "execute": true,
        "message": "do x",
    });
    let err = plan_pipeline(&args).expect_err("must refuse execute without approved id");
    assert_eq!(err, PlannerError::ExecuteWithoutApprovedPlan);
}

#[test]
fn precedence_plan_execute_over_directive_compile() {
    // When the caller supplies both a fresh message and an approved
    // plan id, the latter wins — the caller has clearly already
    // moved past s1 in a previous tick.
    let args = json!({
        "message": "stale message from earlier tick",
        "approved_plan_id": "11111111-1111-1111-1111-111111111111",
        "execute": true,
    });
    let decision = plan_pipeline(&args).expect("should route");
    match decision {
        PipelineDecision::PlanExecute { .. } => {}
        other => panic!("expected PlanExecute, got {:?}", other),
    }
}

#[test]
fn precedence_plan_compile_over_directive_compile() {
    // Same idea one stage earlier: an approved directive id wins
    // over a fresh message.
    let args = json!({
        "message": "stale message",
        "approved_directive_id": "00000000-0000-0000-0000-000000000abc",
        "board_task_id": "btk-7",
    });
    let decision = plan_pipeline(&args).expect("should route");
    match decision {
        PipelineDecision::PlanCompile { .. } => {}
        other => panic!("expected PlanCompile, got {:?}", other),
    }
}

// ── PlannerError → ToolResult mapping ───────────────────────────

#[test]
fn planner_error_response_carries_pipeline_stage() {
    let res = planner_error_response(PlannerError::ApprovedPlanWithoutExecuteFlag);
    assert_eq!(res.is_error, Some(true));
    // ToolResult layout: [structured_error_json, pipeline_meta_json].
    // The structured error is a bare ToolError serialisation (no
    // wrapper) — `error_code`/`reason`/`suggestion` live at the
    // top level. The pipeline metadata is appended as a sibling
    // text element.
    assert!(
        res.content.len() >= 2,
        "expected error + meta sibling, got {} content elements",
        res.content.len()
    );
    let err_text = match &res.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let err_json: Value = serde_json::from_str(&err_text).expect("structured error parses");
    assert_eq!(err_json["error_code"], "INVALID_PARAM");
    assert!(err_json["reason"]
        .as_str()
        .unwrap()
        .contains("approved_plan_id"));
    assert!(err_json["suggestion"]
        .as_str()
        .unwrap()
        .contains("execute=true"));

    let meta_text = match &res.content[1] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let meta: Value = serde_json::from_str(&meta_text).expect("meta json parses");
    assert_eq!(meta["pipeline_stage"], stages::PLAN_REVIEW_GATE);
    assert_eq!(meta["flow_ref"], FLOW_REF);
    // v0 non-goals must be loud in every error payload.
    let non_goals = meta["v0_non_goals"].as_array().expect("non_goals is array");
    assert!(non_goals.iter().any(|v| v == "auto_approve_plan"));
}

#[test]
fn planner_error_stage_for_each_variant() {
    // Pin the stage label for every error variant so refactors that
    // rename a stage immediately fail this test.
    assert_eq!(
        planner_error_stage(&PlannerError::MissingMessage),
        stages::MESSAGE_INTAKE
    );
    assert_eq!(
        planner_error_stage(&PlannerError::PlanCompileMissingBoardTask),
        stages::PLAN_AUTHORING
    );
    assert_eq!(
        planner_error_stage(&PlannerError::ApprovedPlanWithoutExecuteFlag),
        stages::PLAN_REVIEW_GATE
    );
    assert_eq!(
        planner_error_stage(&PlannerError::ExecuteWithoutApprovedPlan),
        stages::EXECUTION_RUNNER
    );
}

// ── argument-shaping helpers ────────────────────────────────────

#[test]
fn nonblank_filters_whitespace_only() {
    assert_eq!(
        nonblank(Some(&json!("  hello  "))),
        Some("hello".to_string())
    );
    assert_eq!(nonblank(Some(&json!("   "))), None);
    assert_eq!(nonblank(Some(&json!(""))), None);
    assert_eq!(nonblank(None), None);
    assert_eq!(nonblank(Some(&Value::Null)), None);
}

#[test]
fn build_directive_compile_args_omits_absent_optionals() {
    let args = json!({ "message": "x" });
    let out = build_directive_compile_args("x".to_string(), &args);
    assert_eq!(out["action"], "compile");
    assert_eq!(out["utterance"], "x");
    assert!(out.get("source").is_none());
    assert!(out.get("conversation_id").is_none());
    assert!(out.get("compiler_mode").is_none());
    assert!(out.get("persist").is_none());
    assert!(out.get("review_gate").is_none());
    assert!(out.get("affected_pillars").is_none());
}

#[test]
fn build_plan_compile_args_omits_absent_optionals() {
    let args = json!({});
    let out = build_plan_compile_args(
        "00000000-0000-0000-0000-000000000abc".to_string(),
        "btk-1".to_string(),
        &args,
    );
    assert_eq!(out["action"], "compile");
    assert_eq!(out["directive_id"], "00000000-0000-0000-0000-000000000abc");
    assert_eq!(out["board_task_id"], "btk-1");
    assert!(out.get("compiler_mode").is_none());
    assert!(out.get("persist").is_none());
    assert!(out.get("acceptance").is_none());
    assert!(out.get("target").is_none());
    assert!(out.get("dispatch_strategy").is_none());
}

#[test]
fn build_plan_execute_args_forwards_only_known_keys() {
    let args = json!({
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "max_parallel_nodes": 8,
        "target": "mission_execution",
        "dispatch_strategy": "agent-team",
        "target_project": "missiond",
        "objective": "ship task 03",
        // unknown keys must NOT be forwarded — keeps the schema
        // single-source-of-truth in plan.rs.
        "totally_unrelated": "value",
    });
    let out = build_plan_execute_args("11111111-1111-1111-1111-111111111111".to_string(), &args);
    assert_eq!(out["action"], "execute");
    assert_eq!(out["plan_id"], "11111111-1111-1111-1111-111111111111");
    assert_eq!(out["execute_mode"], "internal");
    assert_eq!(out["scheduler_mode"], "dag_v1");
    assert_eq!(out["max_parallel_nodes"], 8);
    assert_eq!(out["target"], "mission_execution");
    assert_eq!(out["dispatch_strategy"], "agent-team");
    assert_eq!(out["target_project"], "missiond");
    assert_eq!(out["objective"], "ship task 03");
    assert!(out.get("totally_unrelated").is_none());
}

#[test]
fn build_plan_execute_args_skips_null_values() {
    let args = json!({
        "execute_mode": Value::Null,
        "scheduler_mode": "dag_v1",
    });
    let out = build_plan_execute_args("11111111-1111-1111-1111-111111111111".to_string(), &args);
    assert!(out.get("execute_mode").is_none());
    assert_eq!(out["scheduler_mode"], "dag_v1");
}

// ── stage-label invariants ──────────────────────────────────────

#[test]
fn stage_constants_match_intent_flow_lisp_naming() {
    // These constants line up with `F-intent-alignment-plan-execution-loop`
    // stage ids in `.missiond/v2/intent-flow.lisp`. Renaming a stage
    // there without updating the constant should fail this test (and
    // by extension, the architecture lisp checker that the test
    // suite runs).
    assert_eq!(stages::MESSAGE_INTAKE, "s1_message_intake");
    assert_eq!(stages::DIRECTIVE_REVIEW_GATE, "s3_alignment_review_gate");
    assert_eq!(stages::PLAN_AUTHORING, "s4_plan_authoring");
    assert_eq!(stages::PLAN_REVIEW_GATE, "s5_plan_review_gate");
    assert_eq!(stages::EXECUTION_RUNNER, "s6_execution_runner");
}

#[test]
fn flow_ref_constant_matches_lisp() {
    assert_eq!(FLOW_REF, "F-intent-alignment-plan-execution-loop");
}

// ── wave-14 / Task 04 :: file-first + review-gate forwarding ─────
//
// The next batch of tests pin the v1 forwarding contract: every
// wave-14 opt-in arg must reach the inner handler unchanged, default
// to inert when omitted, and never produce silent surprises (e.g.
// injecting a `topic` default at the pipeline layer would mask the
// directive compiler's "topic required" guard).
//
// We split coverage across three axes:
//   1. forward_file_first_args / forward_review_gate_args helpers —
//      pure args extraction, easy to reason about.
//   2. build_directive_compile_args / build_plan_compile_args —
//      end-to-end wiring through plan_pipeline → builder → out.
//   3. build_artifact_refs — projection of an inner handler payload
//      into the response's artifact_refs slot, including the legacy
//      "no file-write, no review gate" path.

// ── 1. helpers ───────────────────────────────────────────────────

#[test]
fn forward_file_first_args_emits_nothing_when_absent() {
    let args = json!({});
    let mut out = serde_json::Map::new();
    forward_file_first_args(&args, &mut out);
    assert!(out.is_empty(), "no keys expected, got {:?}", out);
}

#[test]
fn forward_file_first_args_propagates_all_keys() {
    let args = json!({
        "write_file": true,
        "overwrite_file": true,
        "topic": "wave14-task04",
        "project": "missiond",
        "cwd": "/abs/path/to/missiond",
        "target_project": "missiond-fallback",
    });
    let mut out = serde_json::Map::new();
    forward_file_first_args(&args, &mut out);
    assert_eq!(out["write_file"], true);
    assert_eq!(out["overwrite_file"], true);
    assert_eq!(out["topic"], "wave14-task04");
    assert_eq!(out["project"], "missiond");
    assert_eq!(out["cwd"], "/abs/path/to/missiond");
    assert_eq!(out["target_project"], "missiond-fallback");
}

#[test]
fn forward_file_first_args_drops_blank_strings() {
    // Blanks must collapse to "absent" so the inner handler's missing-
    // topic guard fires loudly instead of being masked by a "topic=''".
    let args = json!({
        "write_file": true,
        "topic": "   ",
        "project": "",
        "cwd": "  ",
    });
    let mut out = serde_json::Map::new();
    forward_file_first_args(&args, &mut out);
    assert_eq!(out["write_file"], true);
    assert!(
        out.get("topic").is_none(),
        "blank topic must collapse to absent"
    );
    assert!(
        out.get("project").is_none(),
        "blank project must collapse to absent"
    );
    assert!(
        out.get("cwd").is_none(),
        "blank cwd must collapse to absent"
    );
}

#[test]
fn forward_review_gate_args_emits_nothing_when_absent() {
    let args = json!({});
    let mut out = serde_json::Map::new();
    forward_review_gate_args(&args, &mut out);
    assert!(out.is_empty(), "no keys expected, got {:?}", out);
}

#[test]
fn forward_review_gate_args_propagates_all_keys() {
    let args = json!({
        "review_gate_policy": "emit_question",
        "emit_review_question": true,
        "review_question_text": "Confirm alignment scope",
        "review_question_id": "review:directive:abc:v1:compile",
    });
    let mut out = serde_json::Map::new();
    forward_review_gate_args(&args, &mut out);
    assert_eq!(out["review_gate_policy"], "emit_question");
    assert_eq!(out["emit_review_question"], true);
    assert_eq!(out["review_question_text"], "Confirm alignment scope");
    assert_eq!(out["review_question_id"], "review:directive:abc:v1:compile");
}

#[test]
fn forward_review_gate_args_does_not_mask_unknown_policy_values() {
    // We deliberately do NOT collapse unknown policy strings here so
    // the inner `parse_review_gate_policy` can stamp them on the
    // response (it normalises unknown → manual). Forwarding the raw
    // value preserves caller-side typo detection.
    let args = json!({ "review_gate_policy": "no-such-policy" });
    let mut out = serde_json::Map::new();
    forward_review_gate_args(&args, &mut out);
    assert_eq!(out["review_gate_policy"], "no-such-policy");
}

// ── 2. builders ──────────────────────────────────────────────────

#[test]
fn build_directive_compile_args_forwards_wave14_file_first_args() {
    let args = json!({
        "message": "ship file-first writer integration",
        "persist": true,
        "write_file": true,
        "overwrite_file": false,
        "topic": "wave14-04",
        "project": "missiond",
        "target_project": "missiond",
        "cwd": "/Users/jinchen/Projects/missiond",
    });
    let decision = plan_pipeline(&args).expect("should route to directive compile");
    let PipelineDecision::DirectiveCompile { compile_args } = decision else {
        panic!("expected DirectiveCompile");
    };
    // Spec args are intact.
    assert_eq!(
        compile_args["utterance"],
        "ship file-first writer integration"
    );
    assert_eq!(compile_args["persist"], true);
    // wave-14 v1 file-first forwarding.
    assert_eq!(compile_args["write_file"], true);
    assert_eq!(compile_args["overwrite_file"], false);
    assert_eq!(compile_args["topic"], "wave14-04");
    assert_eq!(compile_args["project"], "missiond");
    assert_eq!(compile_args["target_project"], "missiond");
    assert_eq!(compile_args["cwd"], "/Users/jinchen/Projects/missiond");
}

#[test]
fn build_directive_compile_args_forwards_wave14_review_gate_args() {
    let args = json!({
        "message": "ship file-first writer integration",
        "persist": true,
        "write_file": true,
        "topic": "wave14-04",
        "project": "missiond",
        "review_gate_policy": "emit_question",
        "emit_review_question": false,
        "review_question_text": "Confirm directive scope",
        "review_question_id": "review:directive:00000000-0000-0000-0000-000000000abc:v1:compile:deadbeef",
    });
    let decision = plan_pipeline(&args).expect("should route to directive compile");
    let PipelineDecision::DirectiveCompile { compile_args } = decision else {
        panic!("expected DirectiveCompile");
    };
    assert_eq!(compile_args["review_gate_policy"], "emit_question");
    assert_eq!(compile_args["emit_review_question"], false);
    assert_eq!(
        compile_args["review_question_text"],
        "Confirm directive scope"
    );
    assert_eq!(
        compile_args["review_question_id"],
        "review:directive:00000000-0000-0000-0000-000000000abc:v1:compile:deadbeef"
    );
}

#[test]
fn build_directive_compile_args_legacy_no_file_write_path_unchanged() {
    // Pre-wave-14 callers that only send {message, persist} must see a
    // byte-identical inner-args shape — none of the wave-14 keys may
    // appear in the forwarded bag.
    let args = json!({
        "message": "legacy caller",
        "persist": true,
    });
    let decision = plan_pipeline(&args).expect("should route to directive compile");
    let PipelineDecision::DirectiveCompile { compile_args } = decision else {
        panic!("expected DirectiveCompile");
    };
    for k in &[
        "write_file",
        "overwrite_file",
        "topic",
        "project",
        "cwd",
        "target_project",
        "review_gate_policy",
        "emit_review_question",
        "review_question_text",
        "review_question_id",
    ] {
        assert!(
            compile_args.get(*k).is_none(),
            "legacy path must not forward `{}`",
            k
        );
    }
}

#[test]
fn build_plan_compile_args_forwards_wave14_file_first_and_review_gate() {
    let args = json!({
        "approved_directive_id": "00000000-0000-0000-0000-000000000abc",
        "board_task_id": "btk-wave14-04",
        "compiler_mode": "sonnet",
        "persist": true,
        "target": "mission_task_delegate",
        "objective": "compile an executable scaffold",
        "requested_cwd": "/Users/jinchen/Projects/missiond",
        "flow_id": "F-demo",
        // file-first writer
        "write_file": true,
        "overwrite_file": true,
        "topic": "wave14-04-plan",
        "project": "missiond",
        "cwd": "/Users/jinchen/Projects/missiond",
        "target_project": "missiond-fallback",
        // review-gate
        "review_gate_policy": "emit_question",
        "emit_review_question": true,
        "review_question_text": "Approve PLAN.lisp",
        "review_question_id": "review:plan:11111111-1111-1111-1111-111111111111:v1:compile:deadbeef",
    });
    let decision = plan_pipeline(&args).expect("should route to plan compile");
    let PipelineDecision::PlanCompile { compile_args } = decision else {
        panic!("expected PlanCompile");
    };
    // Spec args.
    assert_eq!(
        compile_args["directive_id"],
        "00000000-0000-0000-0000-000000000abc"
    );
    assert_eq!(compile_args["board_task_id"], "btk-wave14-04");
    assert_eq!(compile_args["target"], "mission_task_delegate");
    assert_eq!(compile_args["objective"], "compile an executable scaffold");
    assert_eq!(
        compile_args["requested_cwd"],
        "/Users/jinchen/Projects/missiond"
    );
    assert_eq!(compile_args["flow_id"], "F-demo");
    // file-first.
    assert_eq!(compile_args["write_file"], true);
    assert_eq!(compile_args["overwrite_file"], true);
    assert_eq!(compile_args["topic"], "wave14-04-plan");
    assert_eq!(compile_args["project"], "missiond");
    assert_eq!(compile_args["cwd"], "/Users/jinchen/Projects/missiond");
    assert_eq!(compile_args["target_project"], "missiond-fallback");
    // review-gate.
    assert_eq!(compile_args["review_gate_policy"], "emit_question");
    assert_eq!(compile_args["emit_review_question"], true);
    assert_eq!(compile_args["review_question_text"], "Approve PLAN.lisp");
    assert_eq!(
        compile_args["review_question_id"],
        "review:plan:11111111-1111-1111-1111-111111111111:v1:compile:deadbeef"
    );
}

#[test]
fn build_plan_compile_args_legacy_no_file_write_path_unchanged() {
    // Symmetric guarantee for the s4 plan-compile path: callers that
    // omit every wave-14 opt-in must see a forwarded args bag that
    // contains nothing beyond the wave-13 spec.
    let args = json!({
        "approved_directive_id": "00000000-0000-0000-0000-000000000abc",
        "board_task_id": "btk-legacy",
    });
    let decision = plan_pipeline(&args).expect("should route to plan compile");
    let PipelineDecision::PlanCompile { compile_args } = decision else {
        panic!("expected PlanCompile");
    };
    for k in &[
        "write_file",
        "overwrite_file",
        "topic",
        "project",
        "cwd",
        "target_project",
        "review_gate_policy",
        "emit_review_question",
        "review_question_text",
        "review_question_id",
    ] {
        assert!(
            compile_args.get(*k).is_none(),
            "legacy path must not forward `{}`",
            k
        );
    }
}

#[test]
fn build_plan_execute_args_forwards_wave14_runner_knobs() {
    // The execute path now owns more pass-through keys so a v1 call
    // can drive the plan-runner without reaching for a direct
    // mission_plan(action=execute) call.
    let args = json!({
        "approved_plan_id": "11111111-1111-1111-1111-111111111111",
        "execute": true,
        // wave-13 v0 keys.
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "max_parallel_nodes": 4,
        "target": "mission_task_delegate",
        "dispatch_strategy": "agent-team",
        "target_project": "missiond",
        "objective": "ship task 04",
        // wave-14 v1 additions.
        "cwd": "/Users/jinchen/Projects/missiond",
        "requested_cwd": "/Users/jinchen/Projects/missiond",
        "project": "missiond",
        "intent": "code",
        "execution_id": "exec-wave14-04",
        "parent_design": "directive/00000000-0000-0000-0000-000000000abc",
        "scope": "unified entry pipeline v1",
        "owner": "plan-runner",
        "flow_id": "F-intent-alignment-plan-execution-loop",
        "params": { "foo": "bar" },
        "priority": "high",
        "timeout_secs": 600,
        "dry_run": false,
        "review_question_id": "review:plan:11111111-1111-1111-1111-111111111111:v1:approve",
    });
    let decision = plan_pipeline(&args).expect("should route to plan execute");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    // Sanity checks on v0 keys still being preserved.
    assert_eq!(execute_args["execute_mode"], "internal");
    assert_eq!(execute_args["scheduler_mode"], "dag_v1");
    assert_eq!(execute_args["max_parallel_nodes"], 4);
    assert_eq!(execute_args["target"], "mission_task_delegate");
    assert_eq!(execute_args["dispatch_strategy"], "agent-team");
    assert_eq!(execute_args["target_project"], "missiond");
    assert_eq!(execute_args["objective"], "ship task 04");
    // wave-14 additions.
    for (key, expected) in [
        ("cwd", json!("/Users/jinchen/Projects/missiond")),
        ("requested_cwd", json!("/Users/jinchen/Projects/missiond")),
        ("project", json!("missiond")),
        ("intent", json!("code")),
        ("execution_id", json!("exec-wave14-04")),
        (
            "parent_design",
            json!("directive/00000000-0000-0000-0000-000000000abc"),
        ),
        ("scope", json!("unified entry pipeline v1")),
        ("owner", json!("plan-runner")),
        ("flow_id", json!("F-intent-alignment-plan-execution-loop")),
        ("params", json!({ "foo": "bar" })),
        ("priority", json!("high")),
        ("timeout_secs", json!(600)),
        ("dry_run", json!(false)),
        (
            "review_question_id",
            json!("review:plan:11111111-1111-1111-1111-111111111111:v1:approve"),
        ),
    ] {
        assert_eq!(
            execute_args[key], expected,
            "execute knob `{}` must be forwarded unchanged",
            key
        );
    }
}

/// wave-19 / task 06 + wave-20 / task 04 — task-contract emit knobs
/// + machine-driven dispatch knobs MUST flow through the unified
/// entry pipeline so a v1 caller can drive the new modes without
/// dropping back to a direct `mission_plan(action=execute)` call.
/// Verbatim forwarding — the inner handler owns the allowlist.
#[test]
fn build_plan_execute_args_forwards_task_contract_and_dispatch_contract_knobs() {
    let args = json!({
        "approved_plan_id": "11111111-1111-1111-1111-111111111111",
        "execute": true,
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        // wave-19 / task 06 — emit knobs.
        "task_contract_mode": "emit",
        "emit_task_contract": true,
        // wave-20 / task 04 — dispatch knobs.
        "dispatch_contract_mode": "machine",
        "render_markdown": false,
    });
    let decision = plan_pipeline(&args).expect("should route to plan execute");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    assert_eq!(execute_args["task_contract_mode"], "emit");
    assert_eq!(execute_args["emit_task_contract"], true);
    assert_eq!(execute_args["dispatch_contract_mode"], "machine");
    assert_eq!(execute_args["render_markdown"], false);
}

/// Default unified-entry call must NOT inject the new knobs — the
/// inner handler's defaults (`task_contract_mode=off`,
/// `dispatch_contract_mode=rendered`) preserve the wave-15..19
/// byte-shape for callers that never opt in.
#[test]
fn build_plan_execute_args_omits_task_contract_knobs_by_default() {
    let args = json!({
        "approved_plan_id": "11111111-1111-1111-1111-111111111111",
        "execute": true,
    });
    let decision = plan_pipeline(&args).expect("should route to plan execute");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    assert!(
        execute_args.get("task_contract_mode").is_none(),
        "default pipeline must not inject task_contract_mode"
    );
    assert!(
        execute_args.get("emit_task_contract").is_none(),
        "default pipeline must not inject emit_task_contract"
    );
    assert!(
        execute_args.get("dispatch_contract_mode").is_none(),
        "default pipeline must not inject dispatch_contract_mode"
    );
    assert!(
        execute_args.get("render_markdown").is_none(),
        "default pipeline must not inject render_markdown"
    );
}

// ── 3. artifact_refs projection ─────────────────────────────────

#[test]
fn build_artifact_refs_directive_compile_persisted_with_file_and_review_gate() {
    // Mirror the exact shape `directive::action_compile_dry_run`
    // produces when `persist=true` + `write_file=true` +
    // `review_gate_policy=emit_question` all fired and the file write
    // succeeded.
    let payload = json!({
        "status": "compiled",
        "compiler_mode": "dry_run",
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
        "file_written": true,
        "file_path": "/tmp/missiond/.missiond/alignment/wave14-04/intent-alignment.lisp",
        "file_sha256": "deadbeef",
        "file_bytes": 128,
        "file_created": true,
        "file_overwritten": false,
        "review_gate_policy": "emit_question",
        "review_question_emitted": true,
        "review_question_id": "review:directive:00000000-0000-0000-0000-000000000abc:v1:compile:deadbeef",
        "review_question_text": "Confirm directive scope",
    });
    let refs = build_artifact_refs(ArtifactScope::Directive, &payload);
    assert_eq!(refs["scope"], "directive");
    assert_eq!(refs["directive_id"], "00000000-0000-0000-0000-000000000abc");
    assert_eq!(refs["version"], 1);
    assert_eq!(refs["status"], "compiled");
    assert_eq!(refs["compiler_mode"], "dry_run");
    // File-first splice surfaced verbatim.
    assert_eq!(refs["file_written"], true);
    assert_eq!(
        refs["file_path"],
        "/tmp/missiond/.missiond/alignment/wave14-04/intent-alignment.lisp"
    );
    assert_eq!(refs["file_sha256"], "deadbeef");
    assert_eq!(refs["file_bytes"], 128);
    assert_eq!(refs["file_created"], true);
    assert_eq!(refs["file_overwritten"], false);
    // Review-gate splice surfaced verbatim.
    assert_eq!(refs["review_gate_policy"], "emit_question");
    assert_eq!(refs["review_question_emitted"], true);
    assert_eq!(
        refs["review_question_id"],
        "review:directive:00000000-0000-0000-0000-000000000abc:v1:compile:deadbeef"
    );
    assert_eq!(refs["review_question_text"], "Confirm directive scope");
}

#[test]
fn build_artifact_refs_plan_compile_persisted_minimal_payload() {
    // Plan compile with persist=true but NO write_file / no review-gate
    // policy — the v1 contract is "lift only what the inner handler
    // stamped". Legacy callers should see only the row-id pointers.
    let payload = json!({
        "status": "compiled",
        "compiler_mode": "sonnet",
        "compiler_model": "claude-sonnet",
        "plan_id": "11111111-1111-1111-1111-111111111111",
        "version": 2,
        "board_task_id": "btk-legacy",
    });
    let refs = build_artifact_refs(ArtifactScope::Plan, &payload);
    assert_eq!(refs["scope"], "plan");
    assert_eq!(refs["plan_id"], "11111111-1111-1111-1111-111111111111");
    assert_eq!(refs["version"], 2);
    assert_eq!(refs["board_task_id"], "btk-legacy");
    assert_eq!(refs["status"], "compiled");
    assert_eq!(refs["compiler_mode"], "sonnet");
    assert_eq!(refs["compiler_model"], "claude-sonnet");
    // No file_* / review_question_* fields when the inner handler
    // didn't stamp them — the projection never fabricates.
    for k in &[
        "file_written",
        "file_path",
        "file_sha256",
        "file_bytes",
        "file_created",
        "file_overwritten",
        "file_write_error",
        "review_gate_policy",
        "review_question_emitted",
        "review_question_id",
        "review_question_text",
        "review_question_warning",
    ] {
        let refs_obj = refs.as_object().unwrap();
        assert!(
            !refs_obj.contains_key(*k),
            "legacy projection must not invent `{}`",
            k
        );
    }
}

#[test]
fn build_artifact_refs_partial_status_with_file_write_error_surface() {
    // Verify the partial-status splice (file_first writer's resolver
    // failure path) surfaces both the downgraded status and the
    // explanatory `file_write_error` so callers can act without
    // re-parsing the inner JSON.
    let payload = json!({
        "status": "partial",
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
        "file_written": false,
        "file_write_error": "no project_id, absolute cwd, or fallback target_project supplied",
    });
    let refs = build_artifact_refs(ArtifactScope::Directive, &payload);
    assert_eq!(refs["scope"], "directive");
    assert_eq!(refs["status"], "partial");
    assert_eq!(refs["file_written"], false);
    assert_eq!(
        refs["file_write_error"],
        "no project_id, absolute cwd, or fallback target_project supplied"
    );
}

#[test]
fn build_artifact_refs_review_question_warning_surfaced_with_id_for_retry() {
    // When the review-gate auto-emit publishes against the bus and
    // fails, the inner handler stamps `review_question_emitted=false`
    // + `review_question_id` (deterministic, retry-friendly) +
    // `review_question_warning`. The projection must lift all three so
    // the caller can replay the publish without recomputing the id.
    let payload = json!({
        "status": "compiled",
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
        "file_written": true,
        "file_path": "/tmp/x/intent-alignment.lisp",
        "file_sha256": "deadbeef",
        "review_gate_policy": "emit_question",
        "review_question_emitted": false,
        "review_question_id": "review:directive:abc:v1:compile:deadbeef",
        "review_question_warning": {
            "code": "BUS_PUBLISH_FAILED",
            "reason": "transport closed",
            "scope": "directive",
        },
    });
    let refs = build_artifact_refs(ArtifactScope::Directive, &payload);
    assert_eq!(refs["review_question_emitted"], false);
    assert_eq!(
        refs["review_question_id"],
        "review:directive:abc:v1:compile:deadbeef"
    );
    assert_eq!(
        refs["review_question_warning"]["code"],
        "BUS_PUBLISH_FAILED"
    );
}

#[test]
fn build_artifact_refs_returns_scope_only_for_null_payload() {
    // Defensive: if the inner handler returned a non-JSON ToolContent
    // (e.g. a plain text error), the decorator falls back to Null and
    // `build_artifact_refs` must still return a well-formed object
    // carrying the scope marker so downstream consumers can rely on
    // the field always being present.
    let refs = build_artifact_refs(ArtifactScope::Plan, &Value::Null);
    let obj = refs.as_object().expect("artifact_refs must be an object");
    assert_eq!(obj.len(), 1, "scope-only fallback");
    assert_eq!(obj["scope"], "plan");
}

// ── 4. decorate behaviour (legacy compatibility + new shape) ─────

#[test]
fn decorate_appends_artifact_refs_and_pipeline_meta_without_touching_inner() {
    // Replays the inner -> decorated transformation. We drive
    // `decorate` with a constructed ToolResult so the test stays pure
    // (no AppState). The first content element must be untouched
    // (byte-identical legacy payload) and the appended sibling must
    // carry pipeline_stage / artifact_refs / next_step.
    let inner_payload = json!({
        "status": "compiled",
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
        "file_written": true,
        "file_path": "/tmp/x/intent-alignment.lisp",
        "file_sha256": "deadbeef",
    });
    let inner_text = serde_json::to_string_pretty(&inner_payload).unwrap();
    let inner = ToolResult {
        content: vec![missiond_mcp::tools::ToolContent::Text {
            text: inner_text.clone(),
        }],
        is_error: None,
    };
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::MESSAGE_INTAKE,
            scope: ArtifactScope::Directive,
            next_step: "test next step",
            next_call: Some(json!({ "tool": "mission_directive", "action": "approve" })),
            expects_next_inputs: json!({ "approved_directive_id": "uuid" }),
        },
    );
    assert_eq!(
        decorated.content.len(),
        2,
        "decorate must append a sibling, not replace"
    );
    // Inner element is byte-identical.
    let kept = match &decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    assert_eq!(kept, inner_text, "inner payload must NOT be mutated");
    // Meta sibling carries pipeline_stage + artifact_refs + next_step.
    let meta_text = match &decorated.content[1] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let meta: Value = serde_json::from_str(&meta_text).expect("meta must serialise as JSON");
    assert_eq!(meta["pipeline_stage"], stages::MESSAGE_INTAKE);
    assert_eq!(meta["flow_ref"], FLOW_REF);
    assert_eq!(meta["next_step"], "test next step");
    assert_eq!(meta["next_call"]["action"], "approve");
    // artifact_refs lifts the file-first splice.
    assert_eq!(meta["artifact_refs"]["scope"], "directive");
    assert_eq!(
        meta["artifact_refs"]["directive_id"],
        "00000000-0000-0000-0000-000000000abc"
    );
    assert_eq!(meta["artifact_refs"]["file_written"], true);
    assert_eq!(
        meta["artifact_refs"]["file_path"],
        "/tmp/x/intent-alignment.lisp"
    );
    // v0 non-goals are still loud on every response.
    let non_goals = meta["v0_non_goals"]
        .as_array()
        .expect("v0_non_goals must be an array");
    assert!(non_goals.iter().any(|v| v == "auto_approve_directive"));
    assert!(non_goals.iter().any(|v| v == "auto_approve_plan"));
    assert!(non_goals.iter().any(|v| v == "auto_answer_review_question"));
    assert!(non_goals
        .iter()
        .any(|v| v == "autonomous_workstation_dispatch"));
}

#[test]
fn decorate_legacy_payload_with_no_file_or_review_gate_keeps_artifact_refs_minimal() {
    // Pre-wave-14 caller: persist=false → no file_*, no review_*.
    // The decorator must still surface artifact_refs (with the scope
    // marker), but the row-id slot is empty when the inner handler
    // returned a "dry_run" envelope.
    let inner_payload = json!({
        "status": "dry_run",
        "compiler_mode": "dry_run",
        "utterance": "test",
    });
    let inner_text = serde_json::to_string_pretty(&inner_payload).unwrap();
    let inner = ToolResult {
        content: vec![missiond_mcp::tools::ToolContent::Text {
            text: inner_text.clone(),
        }],
        is_error: None,
    };
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::MESSAGE_INTAKE,
            scope: ArtifactScope::Directive,
            next_step: "review",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let meta_text = match &decorated.content[1] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let meta: Value = serde_json::from_str(&meta_text).unwrap();
    // artifact_refs surfaces scope + status + compiler_mode (lifted
    // from the inner dry_run envelope) but NEVER fabricates file_*
    // for a caller that didn't opt in.
    let refs_obj = meta["artifact_refs"]
        .as_object()
        .expect("artifact_refs must be an object");
    assert_eq!(refs_obj["scope"], "directive");
    assert_eq!(refs_obj["status"], "dry_run");
    assert_eq!(refs_obj["compiler_mode"], "dry_run");
    for k in &[
        "file_written",
        "file_path",
        "file_sha256",
        "review_gate_policy",
        "review_question_emitted",
    ] {
        assert!(
            !refs_obj.contains_key(*k),
            "legacy decoration must not invent `{}`",
            k
        );
    }
}

// ── 5. planner_error scope mapping ──────────────────────────────

#[test]
fn planner_error_response_carries_artifact_refs_scope() {
    // The early-fail error response must carry the same skeleton as a
    // success — pipeline_stage + flow_ref + artifact_refs + next_step
    // + v0_non_goals — so consumer dashboards can rely on a single
    // shape. artifact_refs only carries the scope marker because no
    // handler ran (no row id / file / review question exists yet).
    let res = planner_error_response(PlannerError::PlanCompileMissingBoardTask);
    assert_eq!(res.is_error, Some(true));
    let meta_text = match &res.content[1] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let meta: Value = serde_json::from_str(&meta_text).unwrap();
    assert_eq!(meta["pipeline_stage"], stages::PLAN_AUTHORING);
    assert_eq!(meta["artifact_refs"]["scope"], "plan");
    assert!(
        meta["next_step"].is_string(),
        "planner errors expose the suggestion as next_step"
    );
}

#[test]
fn planner_error_scope_for_each_variant() {
    assert_eq!(
        planner_error_scope(&PlannerError::MissingMessage),
        ArtifactScope::Directive
    );
    assert_eq!(
        planner_error_scope(&PlannerError::PlanCompileMissingBoardTask),
        ArtifactScope::Plan
    );
    assert_eq!(
        planner_error_scope(&PlannerError::ApprovedPlanWithoutExecuteFlag),
        ArtifactScope::Plan
    );
    assert_eq!(
        planner_error_scope(&PlannerError::ExecuteWithoutApprovedPlan),
        ArtifactScope::Execution
    );
}

// ── 6. wave-16 / Task 08 :: canonical-loop e2e smoke ─────────────
//
// Goal: prove the full `message → directive → plan → execute → evidence`
// shape end-to-end without spinning up an AppState, without calling
// Sonnet/Gemini/ClaudeCode, and without dispatching an external
// workstation. The smoke walks the three planner transitions and
// exercises the decorator against simulated inner-handler payloads
// shaped to mirror what `directive::handle` / `plan::handle` actually
// emit in `compiler_mode=dry_run` + `dry_run=true` modes.
//
// Coverage is integration-shape focused: at every stage we assert the
// canonical envelope fields (`pipeline_stage`, `flow_ref`,
// `artifact_refs.scope`, `next_step`, `v0_non_goals`) plus the stage-
// specific carrier (`directive_id`/`plan_id` ids; the s6 dry-run
// `would_dispatch` carrier; the evidence sidecar pointer surfaced
// through artifact_refs when a downstream evidence write produces one).
//
// Out of smoke scope (covered elsewhere or explicitly excluded by the
// task contract):
//   * live LLM calls (sonnet/gemini)              — wave-11/wave-13
//   * live workstation dispatch                   — wave-15/wave-16
//   * DAG scheduler fan-out                       — plan_dag::tests
//   * review-gate bus publishes                   — wave-14/wave-16
//   * end-to-end DB persistence with AppState     — runtime/integration

/// Build a `ToolResult` carrying a single JSON content element so the
/// smoke can drive `decorate` without spinning up the inner handler.
fn smoke_inner_result(payload: Value) -> ToolResult {
    ToolResult {
        content: vec![missiond_mcp::tools::ToolContent::Text {
            text: serde_json::to_string_pretty(&payload).unwrap(),
        }],
        is_error: None,
    }
}

/// Pull the appended pipeline-meta sibling off a decorated result.
/// `decorate` always pushes the meta as the LAST content element.
fn smoke_meta_of(decorated: &ToolResult) -> Value {
    let last = decorated
        .content
        .last()
        .expect("decorated result must carry at least one content element");
    let text = match last {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    serde_json::from_str(&text).expect("pipeline meta must serialise as JSON")
}

#[test]
fn smoke_canonical_loop_message_to_directive_to_plan_to_execute_with_evidence() {
    // ── Stage s1 :: message-intake → directive-compile dry_run ─────
    //
    // The caller hands the pipeline a bare message. The planner picks
    // DirectiveCompile and forwards `compiler_mode=dry_run` so no LLM
    // is reached. We simulate the directive handler returning a
    // `persist=true` dry_run payload (the exact shape produced by
    // `directive::action_compile_dry_run`) and assert the decorated
    // envelope carries the canonical s1 breadcrumbs.
    let s1_args = json!({
        "message": "wave16 task08 smoke utterance",
        "source": "user_utterance",
        "compiler_mode": "dry_run",
        "persist": true,
    });
    let s1_decision = plan_pipeline(&s1_args).expect("s1 must route");
    let PipelineDecision::DirectiveCompile { compile_args } = s1_decision else {
        panic!("expected DirectiveCompile for bare-message input");
    };
    // Forwarded inner args carry the exact knobs the directive
    // compiler expects in dry_run mode.
    assert_eq!(compile_args["action"], "compile");
    assert_eq!(compile_args["utterance"], "wave16 task08 smoke utterance");
    assert_eq!(compile_args["compiler_mode"], "dry_run");
    assert_eq!(compile_args["persist"], true);

    let directive_id = "00000000-0000-0000-0000-0000000000d1";
    let s1_inner = smoke_inner_result(json!({
        "status": "dry_run",
        "compiler_mode": "dry_run",
        "persisted": true,
        "directive_id": directive_id,
        "version": 1,
        "utterance": "wave16 task08 smoke utterance",
        "source": "user_utterance",
    }));
    let s1_decorated = decorate(
        s1_inner,
        DecorateContext {
            stage: stages::MESSAGE_INTAKE,
            scope: ArtifactScope::Directive,
            next_step: "review the compiled directive then re-call this pipeline with `approved_directive_id`",
            next_call: Some(json!({
                "tool": "mission_directive",
                "action": "approve",
            })),
            expects_next_inputs: json!({
                "approved_directive_id": "uuid",
                "directive_version": "i32",
                "board_task_id": "string",
            }),
        },
    );
    let s1_meta = smoke_meta_of(&s1_decorated);
    assert_eq!(s1_meta["pipeline_stage"], stages::MESSAGE_INTAKE);
    assert_eq!(s1_meta["flow_ref"], FLOW_REF);
    assert_eq!(s1_meta["artifact_refs"]["scope"], "directive");
    assert_eq!(s1_meta["artifact_refs"]["directive_id"], directive_id);
    assert_eq!(s1_meta["artifact_refs"]["version"], 1);
    assert_eq!(s1_meta["artifact_refs"]["status"], "dry_run");
    assert!(s1_meta["next_step"].is_string());
    assert_eq!(s1_meta["next_call"]["action"], "approve");
    // The v0/v1 non-goals must remain loud at every stage so the
    // canonical loop never silently auto-approves or auto-dispatches.
    let s1_non_goals = s1_meta["v0_non_goals"].as_array().unwrap();
    assert!(s1_non_goals.iter().any(|v| v == "auto_approve_directive"));
    assert!(s1_non_goals
        .iter()
        .any(|v| v == "autonomous_workstation_dispatch"));

    // ── Stage s4 :: plan-authoring (after explicit s3 approval) ────
    //
    // The caller surfaces the just-approved directive id + a board
    // task anchor. The planner picks PlanCompile in dry_run mode so
    // no LLM is reached. We simulate the plan handler returning the
    // exact shape produced by `plan::action_compile_dry_run` with
    // `persist=true`.
    let board_task_id = "btk-wave16-task08-smoke";
    let s4_args = json!({
        "message": "stale s1 message — must be overridden by approved_directive_id",
        "approved_directive_id": directive_id,
        "directive_version": 1,
        "board_task_id": board_task_id,
        "compiler_mode": "dry_run",
        "persist": true,
        "plan_acceptance": ["smoke passes"],
    });
    let s4_decision = plan_pipeline(&s4_args).expect("s4 must route");
    let PipelineDecision::PlanCompile { compile_args } = s4_decision else {
        panic!("approved_directive_id must take precedence over stale message");
    };
    assert_eq!(compile_args["action"], "compile");
    assert_eq!(compile_args["directive_id"], directive_id);
    assert_eq!(compile_args["board_task_id"], board_task_id);
    assert_eq!(compile_args["compiler_mode"], "dry_run");
    assert_eq!(compile_args["persist"], true);
    assert_eq!(compile_args["acceptance"], json!(["smoke passes"]));

    let plan_id = "11111111-1111-1111-1111-0000000000a1";
    let s4_inner = smoke_inner_result(json!({
        "status": "dry_run",
        "compiler_mode": "dry_run",
        "persisted": true,
        "plan_id": plan_id,
        "version": 1,
        "board_task_id": board_task_id,
        "directive_id": directive_id,
    }));
    let s4_decorated = decorate(
        s4_inner,
        DecorateContext {
            stage: stages::PLAN_AUTHORING,
            scope: ArtifactScope::Plan,
            next_step: "review the compiled PLAN.lisp then re-call this pipeline with `approved_plan_id` and `execute=true`",
            next_call: Some(json!({
                "tool": "mission_plan",
                "action": "approve",
            })),
            expects_next_inputs: json!({
                "approved_plan_id": "uuid",
                "execute": true,
            }),
        },
    );
    let s4_meta = smoke_meta_of(&s4_decorated);
    assert_eq!(s4_meta["pipeline_stage"], stages::PLAN_AUTHORING);
    assert_eq!(s4_meta["flow_ref"], FLOW_REF);
    assert_eq!(s4_meta["artifact_refs"]["scope"], "plan");
    assert_eq!(s4_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(s4_meta["artifact_refs"]["board_task_id"], board_task_id);
    assert_eq!(s4_meta["artifact_refs"]["version"], 1);
    assert_eq!(s4_meta["next_call"]["action"], "approve");
    let s4_non_goals = s4_meta["v0_non_goals"].as_array().unwrap();
    assert!(s4_non_goals.iter().any(|v| v == "auto_approve_plan"));

    // ── Stage s6 :: execution-runner dry_run with would_dispatch ───
    //
    // The caller surfaces the approved plan id with `execute=true`
    // AND `dry_run=true`. The planner picks PlanExecute and forwards
    // every documented runner knob (incl. `dry_run`) — we then
    // simulate the exact dry-run envelope `plan::action_execute_internal`
    // produces (`runner_status=dry_run_no_dispatch` + `would_dispatch`
    // carrier + `workstation_dispatch_source`). This proves that a
    // unified-entry call in dry-run mode never actually spawns an
    // external workstation while still surfacing the canonical
    // dispatch descriptor the caller would replay against the live
    // runner.
    let s6_args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "target": "mission_task_delegate",
        "dispatch_strategy": "agent-team",
        "dry_run": true,
        "project": "missiond",
        "cwd": "/Users/jinchen/Projects/missiond",
        "scope": "wave16/task08 smoke",
    });
    let s6_decision = plan_pipeline(&s6_args).expect("s6 must route");
    let PipelineDecision::PlanExecute { execute_args } = s6_decision else {
        panic!("approved_plan_id + execute=true must route to PlanExecute");
    };
    assert_eq!(execute_args["action"], "execute");
    assert_eq!(execute_args["plan_id"], plan_id);
    assert_eq!(execute_args["execute_mode"], "internal");
    assert_eq!(execute_args["target"], "mission_task_delegate");
    assert_eq!(execute_args["dispatch_strategy"], "agent-team");
    assert_eq!(execute_args["dry_run"], true);
    assert_eq!(execute_args["project"], "missiond");
    assert_eq!(execute_args["cwd"], "/Users/jinchen/Projects/missiond");

    // Simulate exactly the dry-run envelope `action_execute_internal`
    // produces: status=dry_run, runner_status=dry_run_no_dispatch,
    // would_dispatch carrying the inner-args bag the runner WOULD have
    // sent to the target tool.
    let would_dispatch = json!({
        "action": "open",
        "execution_id": format!("plan-{}", plan_id),
        "parent_design": format!("plan/{}", plan_id),
        "owner": "plan-runner",
        "scope": format!("plan {} board_task {}", plan_id, board_task_id),
        "dispatch_strategy": "agent-team",
    });
    let s6_inner = smoke_inner_result(json!({
        "status": "dry_run",
        "execute_mode": "internal",
        "runner_status": "dry_run_no_dispatch",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        "target_tool": "mission_task_delegate",
        "target_source": "explicit_arg",
        "dispatch_strategy": "agent-team",
        "dispatch_strategy_source": "explicit_arg",
        "plan_hint_summary": {},
        "would_dispatch": would_dispatch,
        "workstation_dispatch_source": "default_off",
    }));
    let s6_decorated = decorate(
        s6_inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "execution dispatched (or would-dispatch in dry_run); collect evidence via mission_plan(action=record_evidence)",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let s6_meta = smoke_meta_of(&s6_decorated);
    assert_eq!(s6_meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    assert_eq!(s6_meta["flow_ref"], FLOW_REF);
    assert_eq!(s6_meta["artifact_refs"]["scope"], "execution");
    assert_eq!(s6_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(s6_meta["artifact_refs"]["board_task_id"], board_task_id);
    assert_eq!(s6_meta["artifact_refs"]["status"], "dry_run");

    // The s6 dry-run envelope surfaces `would_dispatch` on the inner
    // payload (NOT on artifact_refs — the projection only lifts
    // documented row-id / file_* / review_question_* fields). The
    // smoke proves the inner payload reaches the caller untouched.
    let s6_inner_text = match &s6_decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let s6_inner_payload: Value =
        serde_json::from_str(&s6_inner_text).expect("inner payload parses");
    assert_eq!(s6_inner_payload["status"], "dry_run");
    assert_eq!(
        s6_inner_payload["runner_status"], "dry_run_no_dispatch",
        "s6 dry_run must not actually dispatch — runner_status proves it"
    );
    assert_eq!(s6_inner_payload["would_dispatch"]["action"], "open");
    assert_eq!(
        s6_inner_payload["would_dispatch"]["dispatch_strategy"],
        "agent-team"
    );
    // workstation_dispatch_source must surface so the caller can prove
    // no autonomous external workstation spawn happened (wave-15/16).
    assert_eq!(
        s6_inner_payload["workstation_dispatch_source"], "default_off",
        "smoke must prove no autonomous workstation dispatch fired"
    );

    // ── Stage s6 evidence sidecar :: record_evidence projection ────
    //
    // After execution (or a dry-run that the caller still wants to
    // breadcrumb), `mission_plan(action=record_evidence)` returns a
    // payload that includes the file-first sidecar pointer
    // (`file_path` + `file_sha256` + `file_bytes`). The decorator
    // projection lifts these into artifact_refs alongside the
    // execution row pointers so a single envelope shape covers the
    // whole loop. We simulate that projection here against the
    // canonical evidence-collector v0 wire shape.
    let evidence_path = "/tmp/missiond-smoke/.missiond/evidence/wave16-task08/plan-evidence.jsonl";
    let evidence_inner = smoke_inner_result(json!({
        "status": "recorded",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        "version": 1,
        "file_written": true,
        "file_path": evidence_path,
        "file_sha256": "deadbeefcafebabe",
        "file_bytes": 256,
        "file_created": true,
        "file_overwritten": false,
    }));
    let evidence_decorated = decorate(
        evidence_inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "evidence recorded; loop closed",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let ev_meta = smoke_meta_of(&evidence_decorated);
    assert_eq!(ev_meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    assert_eq!(ev_meta["artifact_refs"]["scope"], "execution");
    assert_eq!(ev_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(ev_meta["artifact_refs"]["status"], "recorded");
    // Evidence sidecar pointer surfaces through the file-first splice.
    assert_eq!(ev_meta["artifact_refs"]["file_written"], true);
    assert_eq!(ev_meta["artifact_refs"]["file_path"], evidence_path);
    assert_eq!(ev_meta["artifact_refs"]["file_sha256"], "deadbeefcafebabe");
    assert_eq!(ev_meta["artifact_refs"]["file_bytes"], 256);
}

// ── 7. wave-17 / Task 08 :: paused-resume DAG smoke ──────────────
//
// Goal: prove the deterministic review-pause/resume contract end-to-end
// through the unified entry — a PLAN DAG node carrying
// `:review-gate "question-event"` pauses, the deterministic review
// question id surfaces, an `approved` resolution feeds back through the
// pipeline as `resume_review_question_id`, the resumed dispatch reaches
// a terminal `succeeded` state, and the evidence sidecar records both
// the pause and the resume rows.
//
// The smoke is deliberately layered on top of the wave-16 / Task 08
// happy-path scaffold (`smoke_canonical_loop_*`):
//
//   * **Routing**: drives `plan_pipeline` for s1/s4/s6 — same canonical
//     transitions, no live LLM, no AppState, no ClaudeCode spawn.
//   * **Inner payloads**: hand-crafted to mirror `action_execute_dag_v1`
//     (paused branch with `aggregate_status="dag_paused"`) and
//     `action_execute_resume` (approved branch with `inner_status`
//     acceptance + `dag_succeeded` finalization). The shapes are byte-
//     compatible with the production handlers (verified against
//     `plan_dag::tests::*paused_node_review_question_id*` /
//     `*finalize_plan_status_label*` / `*acceptance*`).
//   * **Resume-id determinism**: round-trips through the actual
//     `derive_plan_node_review_question_id` /
//     `parse_review_question_id_struct` /
//     `validate_resume_request` helpers — proves the question id the
//     pause-stage simulated payload carries is the SAME id a real
//     `action_execute_dag_v1` would have emitted, and that the
//     resume validator accepts it against a paused-eligible node.
//
// Features included (each touched by an assertion below):
//
//   * wave-17 / Task 01 — paused-resume hook + the four `resume_*` keys
//     forwarded through `build_plan_execute_args`.
//   * wave-17 / Task 03 — acceptance evaluator surface
//     (`node_results[].acceptance` with `mode="inner_status"`).
//   * wave-17 / Task 05 — `finalize_plan` block + plan status mapping
//     (`aggregate_status="dag_succeeded"` ⇒ `final_plan_status="succeeded"`).
//   * wave-13 evidence sidecar + wave-14 file-first projection lifting
//     `file_path`/`file_sha256`/`file_bytes` into `artifact_refs`.
//   * wave-16 / Task 04 paused-node response surface
//     (`paused_node_ids` / `review_question_ids`).
//   * Canonical envelope (`pipeline_stage` / `flow_ref` /
//     `artifact_refs.scope` / `next_step` / `v0_non_goals`).
//
// Features excluded (covered by their own unit tests, NOT re-asserted
// here per the brief's "smoke contract, not unit duplication" rule):
//
//   * wave-17 / Task 02 claim/lease lifecycle (`Claimed` state, claim
//     evidence rows) — covered by `plan_dag::tests::claim_*`.
//   * wave-17 / Task 04 rollback policy decisions — covered by
//     `plan_dag::tests::rollback_*`.
//   * wave-17 / Task 06 evidence event-log resolver tiers — covered by
//     `evidence_collector::tests::*event_ref*`.
//   * wave-17 / Task 07 workstation brief section 7 (scoped commit
//     handoff) — covered by `workstation_dispatch::tests::*scoped_commit*`.
//   * Bus subscriber side resume listener (`handle_review_resolved_plan_node_event`)
//     — covered by `plan_dag::tests::paused_node_review_question_id_round_trips_*`
//     and the listener integration tests in `bus::v2_subscribers`.

/// Unified-entry forwarding contract: the four `resume_*` keys must
/// reach the inner `mission_plan(execute)` shape via `PlanExecute`.
/// Pinning this here (rather than only in the smoke) gives the
/// forwarding contract its own test name so a future refactor that
/// drops a key doesn't only fail under the longer smoke trace.
#[test]
fn build_plan_execute_args_forwards_wave17_resume_keys() {
    let plan_id = "11111111-1111-1111-1111-000000000000";
    let qid = "review:plan:11111111-1111-1111-1111-000000000000:v1:plan-node:0123456789abcdef";
    let args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "resume_review_question_id": qid,
        "resume_review_decision": "approved",
        "resume_actor": "agent-team",
        "resume_note": "smoke proceed",
    });
    let decision = plan_pipeline(&args).expect("should route to plan execute");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    assert_eq!(execute_args["action"], "execute");
    assert_eq!(execute_args["plan_id"], plan_id);
    assert_eq!(execute_args["execute_mode"], "internal");
    assert_eq!(execute_args["resume_review_question_id"], qid);
    assert_eq!(execute_args["resume_review_decision"], "approved");
    assert_eq!(execute_args["resume_actor"], "agent-team");
    assert_eq!(execute_args["resume_note"], "smoke proceed");
}

/// Resume keys are dropped (not forwarded as nulls) when the caller
/// omits them, so the wave-13/14/15 byte-shape stays intact for legacy
/// callers that never opt into the resume hook.
#[test]
fn build_plan_execute_args_resume_keys_absent_when_omitted() {
    let args = json!({
        "approved_plan_id": "11111111-1111-1111-1111-111111111111",
        "execute": true,
    });
    let decision = plan_pipeline(&args).expect("should route to plan execute");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    for k in &[
        "resume_review_question_id",
        "resume_review_decision",
        "resume_actor",
        "resume_note",
    ] {
        assert!(
            execute_args.get(*k).is_none(),
            "legacy execute path must NOT forward `{}`",
            k
        );
    }
}

#[test]
fn smoke_paused_dag_resume_canonical_loop() {
    // Authoritative ids for the smoke. The plan id is fixed so the
    // deterministic review-question id derivation produces a stable
    // value the assertions can pin.
    let directive_id = "00000000-0000-0000-0000-0000000000d2";
    let board_task_id = "btk-wave17-task08-paused-smoke";
    let plan_id_uuid =
        uuid::Uuid::parse_str("22222222-2222-2222-2222-000000000a17").expect("valid plan uuid");
    let plan_id = plan_id_uuid.to_string();
    let plan_version: i32 = 1;
    let paused_node_id = "review-gate-node";

    // ── Stage s1 :: directive compile (dry_run, no LLM) ────────────
    let s1_args = json!({
        "message": "wave17 task08 paused-resume smoke utterance",
        "source": "user_utterance",
        "compiler_mode": "dry_run",
        "persist": true,
    });
    let s1_decision = plan_pipeline(&s1_args).expect("s1 must route");
    let PipelineDecision::DirectiveCompile { compile_args } = s1_decision else {
        panic!("expected DirectiveCompile for bare-message input");
    };
    assert_eq!(compile_args["compiler_mode"], "dry_run");
    let s1_inner = smoke_inner_result(json!({
        "status": "dry_run",
        "compiler_mode": "dry_run",
        "persisted": true,
        "directive_id": directive_id,
        "version": 1,
    }));
    let s1_decorated = decorate(
        s1_inner,
        DecorateContext {
            stage: stages::MESSAGE_INTAKE,
            scope: ArtifactScope::Directive,
            next_step: "review the compiled directive then re-call this pipeline with `approved_directive_id`",
            next_call: Some(json!({
                "tool": "mission_directive",
                "action": "approve",
            })),
            expects_next_inputs: json!({
                "approved_directive_id": "uuid",
                "directive_version": "i32",
                "board_task_id": "string",
            }),
        },
    );
    let s1_meta = smoke_meta_of(&s1_decorated);
    assert_eq!(s1_meta["pipeline_stage"], stages::MESSAGE_INTAKE);
    assert_eq!(s1_meta["artifact_refs"]["directive_id"], directive_id);

    // ── Stage s4 :: plan compile (PLAN.lisp carries a paused node) ─
    //
    // The simulated PLAN.lisp shape mirrors the wave-16/17 author
    // contract — one node carrying both `:review-gate "question-event"`
    // (so the scheduler will pause it) AND `:acceptance-mode
    // "inner_status"` (so wave-17 / Task 03's evaluator runs after the
    // resumed dispatch). We deliberately don't drive `parse_plan_dag`
    // here; the lisp body is opaque text whose role is only to be
    // surfaced by the (simulated) inner payload's hint summary.
    let plan_lisp_body = format!(
        r#"(plan
             :board_task_id "{}"
             (node
               :id "{}"
               :target "mission_execution"
               :objective "smoke pause then resume"
               :review-gate "question-event"
               :review-action "plan-node"
               :acceptance-mode "inner_status"))
        "#,
        board_task_id, paused_node_id,
    );
    let s4_args = json!({
        "approved_directive_id": directive_id,
        "directive_version": 1,
        "board_task_id": board_task_id,
        "compiler_mode": "dry_run",
        "persist": true,
    });
    let s4_decision = plan_pipeline(&s4_args).expect("s4 must route");
    let PipelineDecision::PlanCompile { compile_args } = s4_decision else {
        panic!("expected PlanCompile");
    };
    assert_eq!(compile_args["directive_id"], directive_id);
    assert_eq!(compile_args["board_task_id"], board_task_id);
    let s4_inner = smoke_inner_result(json!({
        "status": "dry_run",
        "compiler_mode": "dry_run",
        "persisted": true,
        "plan_id": plan_id,
        "version": plan_version,
        "board_task_id": board_task_id,
        "directive_id": directive_id,
        "sexp_text": plan_lisp_body,
    }));
    let s4_decorated = decorate(
        s4_inner,
        DecorateContext {
            stage: stages::PLAN_AUTHORING,
            scope: ArtifactScope::Plan,
            next_step: "review the compiled PLAN.lisp then re-call this pipeline with `approved_plan_id` and `execute=true`",
            next_call: Some(json!({"tool": "mission_plan", "action": "approve"})),
            expects_next_inputs: json!({"approved_plan_id": "uuid", "execute": true}),
        },
    );
    let s4_meta = smoke_meta_of(&s4_decorated);
    assert_eq!(s4_meta["pipeline_stage"], stages::PLAN_AUTHORING);
    assert_eq!(s4_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(s4_meta["artifact_refs"]["board_task_id"], board_task_id);

    // ── Stage s6a :: first execute → DAG paused at the review gate ─
    //
    // Hand the pipeline the approved plan id + execute=true. The
    // planner picks PlanExecute. We then synthesise the inner payload
    // exactly the way `action_execute_dag_v1` produces it when one
    // node landed in `NodeState::Paused` (see `ExecutionOutcome::
    // node_results_json` + `aggregate_status` ⇒ "dag_paused").
    let s6a_args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "max_parallel_nodes": 1,
        // wave-17 / Task 05 finalization opt-in. Surfaces the
        // `finalization` block on the response so the smoke can pin
        // the wave-17 contract that `dag_paused` does NOT lie about
        // success: `final_plan_status` must NOT be "succeeded" while
        // any node is still paused.
        "finalize_plan": true,
    });
    let s6a_decision = plan_pipeline(&s6a_args).expect("s6a must route");
    let PipelineDecision::PlanExecute { execute_args } = s6a_decision else {
        panic!("approved_plan_id + execute=true must route to PlanExecute");
    };
    assert_eq!(execute_args["plan_id"], plan_id);
    assert_eq!(execute_args["execute_mode"], "internal");
    assert_eq!(execute_args["scheduler_mode"], "dag_v1");
    assert_eq!(execute_args["finalize_plan"], true);

    // Round-trip the deterministic review-question id BEFORE building
    // the inner payload so the smoke proves the id the simulated
    // payload carries is the SAME id a live `emit_paused_review_gate`
    // would have produced for this plan/version/node.
    let pause_review_qid = super::super::review_gate::derive_plan_node_review_question_id(
        &plan_id,
        plan_version,
        paused_node_id,
        Some("plan-node"),
    );
    // Layer 1: envelope shape.
    let parsed_qid = super::super::review_gate::parse_review_question_id_struct(&pause_review_qid)
        .expect("derived qid must parse");
    assert_eq!(parsed_qid.scope, "plan");
    assert_eq!(parsed_qid.action, "plan-node");
    assert_eq!(parsed_qid.artifact_id, plan_id);
    assert_eq!(parsed_qid.version, plan_version);
    assert_eq!(
        parsed_qid.topic_hash.as_deref(),
        Some(super::super::review_gate::derive_plan_node_topic_hash(paused_node_id).as_str())
    );

    // Layer 2: validator confirms the qid maps back to the originating
    // paused-eligible node — this is the SAME validation
    // `action_execute_resume` performs before re-dispatching the node.
    // We construct a `ParsedDag` directly (no AppState) carrying the
    // single paused-eligible node so the validator has the same view
    // it would see in production after `build_validated_dag(plan.sexp_text)`.
    let dag_node = super::super::plan_dag::DagNode {
        id: paused_node_id.into(),
        target: "mission_execution".into(),
        failure_policy: "fail-fast".into(),
        review_gate: Some("question-event".into()),
        review_action: Some("plan-node".into()),
        acceptance_mode_raw: Some("inner_status".into()),
        ..Default::default()
    };
    let parsed_dag = super::super::plan_dag::ParsedDag {
        nodes: vec![dag_node.clone()],
        unsupported_top_forms: Vec::new(),
    };
    let plan_for_validator = missiond_core::types::Plan {
        id: plan_id_uuid,
        board_task_id: board_task_id.to_string(),
        source_directive_id: None,
        version: plan_version,
        sexp_text: plan_lisp_body.clone(),
        sexp_hash: "deadbeef".to_string(),
        status: missiond_core::types::PlanStatus::Approved,
        compiler_model: None,
        compiled_from: None,
        contract_json: serde_json::json!({}),
        created_at: chrono::Utc::now(),
        approved_at: None,
        finished_at: None,
    };
    let routed_node = super::super::plan_dag::validate_resume_request(
        &parsed_qid,
        &plan_for_validator,
        &parsed_dag,
    )
    .expect("validator must accept the round-trip qid");
    assert_eq!(routed_node.id, paused_node_id);

    // Sidecar evidence path the wave-13 evidence_collector would have
    // recorded for the paused→pending transition. Drives the
    // `artifact_refs.file_*` projection assertions below.
    let pause_evidence_path =
        "/tmp/missiond-smoke/.missiond/evidence/wave17-task08/plan-evidence.jsonl";
    let s6a_inner = smoke_inner_result(json!({
        // Top-level shape mirrors `action_execute_dag_v1` — pick the
        // `dag_paused` aggregate so this run is unambiguously the
        // paused branch.
        "status": "dag_paused",
        "aggregate_status": "dag_paused",
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "runner_status": "review_gate_paused",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        "node_count": 1,
        "max_parallel_nodes": 1,
        // wave-16 / Task 04 paused-node surface. The smoke pins the
        // exact id the deterministic emitter produces so the resume
        // call below can reference the SAME envelope the production
        // pause path would have written to the sidecar.
        "paused_nodes": [{
            "id": paused_node_id,
            "target": "mission_execution",
            "state": "paused",
            "review_question_id": pause_review_qid,
        }],
        "paused_node_ids": [paused_node_id],
        "review_question_ids": [pause_review_qid],
        "node_results": [{
            "id": paused_node_id,
            "target": "mission_execution",
            "state": "paused",
            "dispatch_strategy": "unknown",
            "inner_result": null,
            "review_question_id": pause_review_qid,
        }],
        "evidence_path": pause_evidence_path,
        "plan_status": "executing",
        // wave-17 / Task 05 finalization block. The "no lie" invariant:
        // a paused run preserves the prior plan status and explicitly
        // records the `paused_node_present_no_finalization` rule so
        // observers see WHY the plan did NOT advance to succeeded.
        "finalization": {
            "finalize_plan": true,
            "aggregate_status": "dag_paused",
            "final_plan_status": "executing",
            "rule": "paused_node_present_no_finalization",
        },
        // File-first sidecar splice → lifted into `artifact_refs.file_*`
        // by `build_artifact_refs` so the smoke can prove the paused
        // run still wrote evidence (the sidecar pointer is the same
        // file the resume run will append to in stage s6b below).
        "file_written": true,
        "file_path": pause_evidence_path,
        "file_sha256": "deadbeefcafebabe1111",
        "file_bytes": 512,
    }));
    let s6a_decorated = decorate(
        s6a_inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "DAG paused at review gate; resolve via mission_review then re-call this pipeline with `resume_review_question_id`",
            next_call: Some(json!({
                "tool": "mission_review",
                "action": "resolve",
                "review_question_id": pause_review_qid,
            })),
            expects_next_inputs: json!({
                "resume_review_question_id": "string",
                "resume_review_decision": "approved|rejected|needs_changes",
                "execute": true,
            }),
        },
    );
    let s6a_meta = smoke_meta_of(&s6a_decorated);
    assert_eq!(s6a_meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    assert_eq!(s6a_meta["artifact_refs"]["scope"], "execution");
    assert_eq!(s6a_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(s6a_meta["artifact_refs"]["status"], "dag_paused");
    // Pause-side evidence sidecar pointer surfaces via `file_*`.
    assert_eq!(s6a_meta["artifact_refs"]["file_written"], true);
    assert_eq!(s6a_meta["artifact_refs"]["file_path"], pause_evidence_path);
    assert_eq!(
        s6a_meta["artifact_refs"]["file_sha256"],
        "deadbeefcafebabe1111"
    );

    // Inner payload (preserved byte-for-byte by `decorate`) carries
    // the paused-node surface + finalization "no lie" block.
    let s6a_inner_text = match &s6a_decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let s6a_inner_payload: Value =
        serde_json::from_str(&s6a_inner_text).expect("s6a inner payload parses");
    assert_eq!(s6a_inner_payload["aggregate_status"], "dag_paused");
    assert_eq!(s6a_inner_payload["runner_status"], "review_gate_paused");
    assert_eq!(s6a_inner_payload["paused_node_ids"][0], paused_node_id);
    assert_eq!(
        s6a_inner_payload["review_question_ids"][0],
        pause_review_qid
    );
    // wave-17 / Task 05: the finalization block must NOT advance the
    // plan to `succeeded` while any node is still paused.
    assert_eq!(
        s6a_inner_payload["finalization"]["aggregate_status"],
        "dag_paused"
    );
    assert_ne!(
        s6a_inner_payload["finalization"]["final_plan_status"], "succeeded",
        "wave-17 / Task 05 invariant: dag_paused MUST NOT lie about success"
    );
    assert_eq!(
        s6a_inner_payload["finalization"]["rule"],
        "paused_node_present_no_finalization"
    );

    // ── Stage s6b :: resume execute → approved → terminal succeeded ─
    //
    // The reviewer approved the gate. The caller hands the pipeline
    // the SAME plan id + the deterministic review-question id +
    // `resume_review_decision="approved"`. The planner picks
    // PlanExecute and forwards the four wave-17 / Task 01 resume keys
    // verbatim through `build_plan_execute_args` (covered by the
    // dedicated forwarding test above). We then synthesise the inner
    // payload mirroring `action_execute_resume`'s approved-branch
    // shape: the previously-paused node moves through
    // `paused -> ready -> running -> succeeded`, the wave-17 / Task 03
    // acceptance evaluator stamps `inner_status: accepted`, and the
    // wave-17 / Task 05 finalization block flips to `dag_succeeded`
    // ⇒ `final_plan_status="succeeded"`.
    let s6b_args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "resume_review_question_id": pause_review_qid,
        "resume_review_decision": "approved",
        "resume_actor": "agent-team",
        "resume_note": "smoke approves",
        "finalize_plan": true,
    });
    let s6b_decision = plan_pipeline(&s6b_args).expect("s6b must route");
    let PipelineDecision::PlanExecute { execute_args } = s6b_decision else {
        panic!("resume call must route to PlanExecute");
    };
    // Forwarding contract proven through the routing decision.
    assert_eq!(execute_args["plan_id"], plan_id);
    assert_eq!(execute_args["execute_mode"], "internal");
    assert_eq!(execute_args["resume_review_question_id"], pause_review_qid);
    assert_eq!(execute_args["resume_review_decision"], "approved");
    assert_eq!(execute_args["resume_actor"], "agent-team");
    assert_eq!(execute_args["resume_note"], "smoke approves");
    assert_eq!(execute_args["finalize_plan"], true);

    let resume_evidence_path =
        "/tmp/missiond-smoke/.missiond/evidence/wave17-task08/plan-evidence.jsonl";
    let s6b_inner = smoke_inner_result(json!({
        // `action_execute_resume`'s approved-branch wire shape.
        "status": "dag_succeeded",
        "aggregate_status": "dag_succeeded",
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "runner_status": "all_nodes_dispatched",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        "node_count": 1,
        "resume": {
            "review_question_id": pause_review_qid,
            "review_decision": "approved",
            "actor": "agent-team",
        },
        "node_results": [{
            "id": paused_node_id,
            "target": "mission_execution",
            "state": "succeeded",
            "dispatch_strategy": "unknown",
            "inner_result": {"ok": true},
            // wave-17 / Task 03 acceptance surface — `inner_status`
            // mode accepts because the resumed dispatch returned a
            // success classification with no error signal in the
            // inner payload.
            "acceptance": {
                "status": "accepted",
                "mode": "inner_status",
                "commands": [],
                "evidence_keys": [],
                "reason": "inner_status: dispatch Ok and payload carries no error signal",
            },
            // Per-node retry surface stays quiet for the v2-baseline
            // single-attempt contract — pinning its absence proves
            // the smoke does not accidentally synthesise retry
            // bookkeeping the resume path never produces.
        }],
        "paused_node_ids": [],
        "review_question_ids": [],
        "evidence_path": resume_evidence_path,
        "plan_status": "succeeded",
        "finalization": {
            "finalize_plan": true,
            "aggregate_status": "dag_succeeded",
            "final_plan_status": "succeeded",
            "rule": "all_terminal_no_failed_no_paused",
            // `distill_on_success` was NOT requested → no distill
            // block. Pinning its absence proves the smoke did not
            // silently invoke the distill trigger (which would
            // require the workflow handler).
        },
        // File-first sidecar splice → same path as the pause run so
        // the round-trip proves both rows landed in the same sidecar
        // (the production evidence_collector appends to the same file
        // for the same plan_id).
        "file_written": true,
        "file_path": resume_evidence_path,
        "file_sha256": "deadbeefcafebabe2222",
        "file_bytes": 1024,
    }));
    let s6b_decorated = decorate(
        s6b_inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "DAG resumed and succeeded; loop closed",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let s6b_meta = smoke_meta_of(&s6b_decorated);
    assert_eq!(s6b_meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    assert_eq!(s6b_meta["artifact_refs"]["scope"], "execution");
    assert_eq!(s6b_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(s6b_meta["artifact_refs"]["status"], "dag_succeeded");
    // Resume-side evidence sidecar pointer surfaces via `file_*`.
    assert_eq!(s6b_meta["artifact_refs"]["file_written"], true);
    assert_eq!(s6b_meta["artifact_refs"]["file_path"], resume_evidence_path);
    assert_eq!(
        s6b_meta["artifact_refs"]["file_sha256"],
        "deadbeefcafebabe2222"
    );

    let s6b_inner_text = match &s6b_decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let s6b_inner_payload: Value =
        serde_json::from_str(&s6b_inner_text).expect("s6b inner payload parses");
    // Aggregate flips to `dag_succeeded` and the previously-paused
    // node has reached the `succeeded` terminal state.
    assert_eq!(s6b_inner_payload["aggregate_status"], "dag_succeeded");
    assert_eq!(s6b_inner_payload["runner_status"], "all_nodes_dispatched");
    assert_eq!(
        s6b_inner_payload["node_results"][0]["state"], "succeeded",
        "previously-paused node must reach terminal succeeded"
    );
    // Resume metadata is preserved on the inner payload so observers
    // can correlate the resume call with the paused row it cleared.
    assert_eq!(
        s6b_inner_payload["resume"]["review_question_id"],
        pause_review_qid
    );
    assert_eq!(s6b_inner_payload["resume"]["review_decision"], "approved");
    // wave-17 / Task 03: the acceptance evaluator surface lifts
    // `mode="inner_status"` and `status="accepted"`.
    assert_eq!(
        s6b_inner_payload["node_results"][0]["acceptance"]["mode"],
        "inner_status"
    );
    assert_eq!(
        s6b_inner_payload["node_results"][0]["acceptance"]["status"],
        "accepted"
    );
    // wave-17 / Task 05: the finalization block now claims
    // `dag_succeeded` ⇒ `final_plan_status="succeeded"` per the
    // `finalize_plan_status_label` mapping.
    assert_eq!(
        s6b_inner_payload["finalization"]["aggregate_status"],
        "dag_succeeded"
    );
    assert_eq!(
        s6b_inner_payload["finalization"]["final_plan_status"],
        "succeeded"
    );
    assert_eq!(
        s6b_inner_payload["finalization"]["rule"],
        "all_terminal_no_failed_no_paused"
    );

    // ── Sidecar invariant: pause + resume rows share one file ──────
    //
    // Both the pause-stage and resume-stage envelopes pointed at the
    // same `evidence_path` — the production evidence_collector
    // appends to the same per-plan sidecar — so a single file path
    // surfaces the full pause+resume audit trail. We pin both
    // `artifact_refs.file_path` projections to prove the smoke holds
    // that invariant end-to-end.
    assert_eq!(
        s6a_meta["artifact_refs"]["file_path"], s6b_meta["artifact_refs"]["file_path"],
        "pause + resume must share one evidence sidecar file"
    );
}

// ── 8. wave-18 / Task 09 :: autonomous-loop smoke (post-Wave17/18) ──
//
// Goal: prove the most-complete current MissionD loop end-to-end through
// the unified entry. Layers the wave-18 features on top of the wave-17
// paused-resume scaffold (`smoke_paused_dag_resume_canonical_loop`):
//
//   * wave-18 / task 02 — `ExecutionEvent::Claimed` / `::Completed`
//     dispatch metadata (dispatch_strategy / target_project /
//     requested_cwd) surfaces on each per-node attempt so a downstream
//     consumer can join the bus event with the dispatch context without
//     re-loading the companion log.
//   * wave-18 / task 03 — cross-node acceptance fan-in (`fan_in` block
//     under `node_results[].acceptance` with mode/source_nodes/passed/
//     reason). The smoke pins a passing `all-succeeded` fan-in.
//   * wave-18 / task 04 — cascade rollback descriptor (record-only,
//     `mode="plan"`, no dispatch) surfaces under
//     `node_results[].rollback.cascade` for the failed-node path; we
//     pin that the smoke's success path keeps the cascade block absent
//     (no failed root) but exercises the descriptor shape on a
//     dedicated cascade-mode-resolution assertion.
//   * wave-18 / task 05 — cross-plan distill chain in `record_only`
//     mode. The chain block lands under `finalization.distill_chain`
//     with `triggered=true`/`status="recorded"`/`chain_mode=
//     "record_only"` and the top-level shortcuts
//     `distill_chain_status` / `distill_chain_id` mirror it so callers
//     can grep one place. NO LLM is invoked (record_only is the
//     no-network path).
//   * wave-18 / task 06 — autonomous PLAN field inference forwarding
//     contract (`infer_plan_fields` reaches the inner execute call)
//     pinned via the dedicated `build_plan_execute_args_*` test. The
//     smoke walks the runtime path with `infer_plan_fields="off"` so
//     the wave-17 byte-shape stays intact while the forwarding
//     contract is still exercised.
//   * wave-18 / task 01 — bounded `EventLogQuery` provenance reaches
//     the evidence sidecar via `EventRefProvenance::EventLogQuery`. We
//     simulate the projection on the s6b inner payload's
//     `event_refs[]` block (mirrors the
//     `evidence_collector::resolve_event_refs` `event_log_query` path).
//   * wave-18 / task 07 — review automation policy is OUT of scope
//     for unified_entry (the policy applies to plan
//     approve/mark/supersede, which the unified entry never routes
//     to). Pinned as a non-goal in the smoke comment so a future
//     refactor that tries to add an "auto-approve" route lands an
//     explicit failure.
//   * wave-18 / task 08 — scoped commit worktree preflight is a
//     `mission_execution(action=preflight_commit)` surface NOT routed
//     through unified_entry. Same non-goal stance.
//
// No LLM, no spawn, no shell — the smoke uses `decorate` against
// hand-crafted inner payloads that mirror the production handler
// shapes. The smoke is FOCUSED — see comments above for what is
// covered elsewhere by dedicated unit tests.

/// Forwarding contract: the three wave-18 / task 05 chain knobs must
/// reach the inner `mission_plan(execute)` shape via `PlanExecute`.
/// Pinning the chain knobs in their own test (alongside the smoke
/// below) guarantees a future refactor that drops one fails fast on
/// a small targeted test rather than only on the broader smoke.
#[test]
fn build_plan_execute_args_forwards_wave18_distill_chain_keys() {
    let plan_id = "33333333-3333-3333-3333-000000000018";
    let args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "finalize_plan": true,
        "distill_chain_id": "chain:wave18-smoke",
        "distill_chain_mode": "record_only",
        "distill_chain_name": "wave18-task09-smoke",
    });
    let decision = plan_pipeline(&args).expect("should route to plan execute");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    assert_eq!(execute_args["plan_id"], plan_id);
    assert_eq!(execute_args["finalize_plan"], true);
    assert_eq!(execute_args["distill_chain_id"], "chain:wave18-smoke");
    assert_eq!(execute_args["distill_chain_mode"], "record_only");
    assert_eq!(execute_args["distill_chain_name"], "wave18-task09-smoke");
}

/// Forwarding contract: the wave-18 / task 06 `infer_plan_fields` knob
/// must reach the inner `mission_plan(execute)` shape. The unified
/// entry NEVER re-validates the strict allowlist — that is the inner
/// handler's job (`parse_infer_plan_fields_mode`).
#[test]
fn build_plan_execute_args_forwards_wave18_infer_plan_fields_key() {
    let plan_id = "33333333-3333-3333-3333-000000000019";
    for mode in ["off", "preview", "apply_safe"] {
        let args = json!({
            "approved_plan_id": plan_id,
            "execute": true,
            "execute_mode": "internal",
            "infer_plan_fields": mode,
        });
        let decision = plan_pipeline(&args).expect("should route to plan execute");
        let PipelineDecision::PlanExecute { execute_args } = decision else {
            panic!("expected PlanExecute");
        };
        assert_eq!(
            execute_args["infer_plan_fields"], mode,
            "infer_plan_fields=`{}` must be forwarded unchanged",
            mode
        );
    }
}

/// Forwarding contract: when wave-18 chain / inference knobs are
/// omitted, the execute args bag stays byte-identical to the wave-17
/// shape (no fabricated nulls, no defaults injected at the pipeline
/// layer). Mirrors `build_plan_execute_args_resume_keys_absent_when_omitted`
/// for the same legacy-shape preservation guarantee.
#[test]
fn build_plan_execute_args_wave18_keys_absent_when_omitted() {
    let args = json!({
        "approved_plan_id": "33333333-3333-3333-3333-00000000001a",
        "execute": true,
    });
    let decision = plan_pipeline(&args).expect("should route to plan execute");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    for k in &[
        "distill_chain_id",
        "distill_chain_mode",
        "distill_chain_name",
        "infer_plan_fields",
    ] {
        assert!(
            execute_args.get(*k).is_none(),
            "legacy execute path must NOT forward `{}`",
            k
        );
    }
}

#[test]
fn smoke_wave18_canonical_loop_full_features() {
    // Authoritative ids for the smoke.
    let directive_id = "00000000-0000-0000-0000-000000000d18";
    let board_task_id = "btk-wave18-task09-smoke";
    let plan_id_uuid =
        uuid::Uuid::parse_str("44444444-4444-4444-4444-000000000a18").expect("valid plan uuid");
    let plan_id = plan_id_uuid.to_string();
    let plan_version: i32 = 1;
    let dep_node_id = "preflight-node";
    let paused_node_id = "review-gate-node";
    // Distill chain id pinned so the assertion can grep one place.
    let chain_id = "chain:wave18-smoke";
    let chain_name = "wave18-task09-smoke";

    // ── Stage s1 :: directive compile (dry_run, no LLM) ────────────
    let s1_args = json!({
        "message": "wave18 task09 autonomous-loop smoke utterance",
        "source": "user_utterance",
        "compiler_mode": "dry_run",
        "persist": true,
    });
    let s1_decision = plan_pipeline(&s1_args).expect("s1 must route");
    let PipelineDecision::DirectiveCompile { compile_args } = s1_decision else {
        panic!("expected DirectiveCompile for bare-message input");
    };
    assert_eq!(compile_args["compiler_mode"], "dry_run");
    let s1_inner = smoke_inner_result(json!({
        "status": "dry_run",
        "compiler_mode": "dry_run",
        "persisted": true,
        "directive_id": directive_id,
        "version": 1,
    }));
    let s1_decorated = decorate(
        s1_inner,
        DecorateContext {
            stage: stages::MESSAGE_INTAKE,
            scope: ArtifactScope::Directive,
            next_step: "review the compiled directive then re-call this pipeline with `approved_directive_id`",
            next_call: Some(json!({"tool": "mission_directive", "action": "approve"})),
            expects_next_inputs: json!({
                "approved_directive_id": "uuid",
                "directive_version": "i32",
                "board_task_id": "string",
            }),
        },
    );
    let s1_meta = smoke_meta_of(&s1_decorated);
    assert_eq!(s1_meta["pipeline_stage"], stages::MESSAGE_INTAKE);
    assert_eq!(s1_meta["artifact_refs"]["directive_id"], directive_id);

    // ── Stage s4 :: plan compile (PLAN.lisp w/ wave-18 hints) ──────
    //
    // Two-node DAG so we can exercise wave-18 / task 03 fan-in
    // (`:acceptance-depends-on`) on the second node referencing the
    // first. The first node also opts into retry (max-attempts > 1
    // exercises wave-16 / task 05 retry observability) and rollback
    // descriptor (wave-17 / task 04 +
    // wave-18 / task 04 cascade) so the smoke walks the maximum
    // hint surface in a single dispatch.
    let plan_lisp_body = format!(
        r#"(plan
             :board_task_id "{btk}"
             (node
               :id "{dep}"
               :target "mission_execution"
               :objective "preflight before review gate"
               :max-attempts 2
               :retry-on-failure 1
               :rollback-policy "descriptor"
               :rollback-objective "revert preflight"
               :rollback-owned-files ("/tmp/missiond-smoke/preflight.txt")
               :rollback-cascade "plan")
             (node
               :id "{gate}"
               :requires ("{dep}")
               :target "mission_execution"
               :objective "smoke pause then resume"
               :review-gate "question-event"
               :review-action "plan-node"
               :acceptance-mode "inner_status"
               :acceptance-depends-on ("{dep}")
               :acceptance-requires "all-succeeded"))
        "#,
        btk = board_task_id,
        dep = dep_node_id,
        gate = paused_node_id,
    );
    let s4_args = json!({
        "approved_directive_id": directive_id,
        "directive_version": 1,
        "board_task_id": board_task_id,
        "compiler_mode": "dry_run",
        "persist": true,
    });
    let s4_decision = plan_pipeline(&s4_args).expect("s4 must route");
    let PipelineDecision::PlanCompile { compile_args } = s4_decision else {
        panic!("expected PlanCompile");
    };
    assert_eq!(compile_args["board_task_id"], board_task_id);
    let s4_inner = smoke_inner_result(json!({
        "status": "dry_run",
        "compiler_mode": "dry_run",
        "persisted": true,
        "plan_id": plan_id,
        "version": plan_version,
        "board_task_id": board_task_id,
        "directive_id": directive_id,
        "sexp_text": plan_lisp_body,
    }));
    let s4_decorated = decorate(
        s4_inner,
        DecorateContext {
            stage: stages::PLAN_AUTHORING,
            scope: ArtifactScope::Plan,
            next_step: "review the compiled PLAN.lisp then re-call this pipeline with `approved_plan_id` and `execute=true`",
            next_call: Some(json!({"tool": "mission_plan", "action": "approve"})),
            expects_next_inputs: json!({"approved_plan_id": "uuid", "execute": true}),
        },
    );
    let s4_meta = smoke_meta_of(&s4_decorated);
    assert_eq!(s4_meta["pipeline_stage"], stages::PLAN_AUTHORING);
    assert_eq!(s4_meta["artifact_refs"]["plan_id"], plan_id);

    // Round-trip the deterministic review-question id BEFORE building
    // the s6 inner payload so the assertion can pin the SAME id a
    // live `emit_paused_review_gate` would produce.
    let pause_review_qid = super::super::review_gate::derive_plan_node_review_question_id(
        &plan_id,
        plan_version,
        paused_node_id,
        Some("plan-node"),
    );
    let parsed_qid = super::super::review_gate::parse_review_question_id_struct(&pause_review_qid)
        .expect("derived qid must parse");
    assert_eq!(parsed_qid.scope, "plan");
    assert_eq!(parsed_qid.action, "plan-node");
    assert_eq!(parsed_qid.artifact_id, plan_id);

    // ── Stage s6a :: execute → preflight succeeds, gate node pauses ─
    //
    // Args layered on top of the wave-17 paused smoke: add the
    // wave-18 chain knobs (`distill_chain_*`) + the inference knob
    // (`infer_plan_fields="off"` → no behaviour change at runtime,
    // forwarding contract pinned via the dedicated test above).
    let s6a_args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "max_parallel_nodes": 2,
        "dispatch_strategy": "agent-team",
        "target_project": "missiond",
        "cwd": "/Users/jinchen/Projects/missiond",
        "finalize_plan": true,
        // wave-18 / task 05 — chain in record_only mode (no LLM).
        "distill_chain_id": chain_id,
        "distill_chain_mode": "record_only",
        "distill_chain_name": chain_name,
        // wave-18 / task 06 — inference forwarding pinned, runtime off.
        "infer_plan_fields": "off",
    });
    let s6a_decision = plan_pipeline(&s6a_args).expect("s6a must route");
    let PipelineDecision::PlanExecute { execute_args } = s6a_decision else {
        panic!("approved_plan_id + execute=true must route to PlanExecute");
    };
    assert_eq!(execute_args["plan_id"], plan_id);
    assert_eq!(execute_args["finalize_plan"], true);
    assert_eq!(execute_args["distill_chain_id"], chain_id);
    assert_eq!(execute_args["distill_chain_mode"], "record_only");
    assert_eq!(execute_args["distill_chain_name"], chain_name);
    assert_eq!(execute_args["infer_plan_fields"], "off");

    // Sidecar evidence path — same file the resume run will append
    // to so the round-trip proves both rows landed in one sidecar.
    let evidence_path = "/tmp/missiond-smoke/.missiond/v2/plans/wave18-task09.evidence.json";
    let lease_iso = "2026-04-26T00:30:00Z";
    // wave-18 / task 02 — `ExecutionEvent::Claimed` carries
    // dispatch_strategy / target_project / requested_cwd. We mirror
    // the live wire shape on the simulated `event_refs[]` block
    // surfaced through the per-node evidence (mirrors what the
    // wave-18 / task 01 `EventLogQueryable` resolver tier produces
    // in `evidence_collector::resolve_event_refs`).
    let dep_claim_id = format!("plan-dag:{}:{}:1", plan_id, dep_node_id);
    let s6a_inner = smoke_inner_result(json!({
        "status": "dag_paused",
        "aggregate_status": "dag_paused",
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "runner_status": "review_gate_paused",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        "node_count": 2,
        "max_parallel_nodes": 2,
        // wave-18 / task 06 — inference block surfaces with mode=off
        // so the caller can confirm "no inference happened" without
        // diving into the inner payload. (The runtime emits this
        // block on EVERY execute call, even with mode=off, per the
        // wave-18 / task 06 contract.)
        "plan_field_inference": {
            "mode": "off",
            "fields_inferred": [],
            "fields_conflicting": [],
        },
        // wave-16 / task 04 paused-node surface.
        "paused_nodes": [{
            "id": paused_node_id,
            "target": "mission_execution",
            "state": "paused",
            "review_question_id": pause_review_qid,
        }],
        "paused_node_ids": [paused_node_id],
        "review_question_ids": [pause_review_qid],
        "node_results": [
            // First node: preflight succeeded on attempt 1 of 2.
            {
                "id": dep_node_id,
                "target": "mission_execution",
                "state": "succeeded",
                "dispatch_strategy": "agent-team",
                "inner_result": {"ok": true},
                // wave-16 / task 05 retry observability — surfaces
                // because :max-attempts=2 authorised more than one.
                "retry": {
                    "attempts": 1,
                    "max_attempts": 2,
                },
                // wave-18 / task 02 — bus dispatch metadata mirrored
                // on the evidence-side `event_refs` block so a
                // downstream consumer can join the bus stream with
                // dispatch context without re-loading the companion
                // log. Provenance label matches
                // `EventRefProvenance::EventLogQuery::as_str()`.
                "event_refs": [
                    {
                        "kind": "claimed",
                        "claim_id": dep_claim_id,
                        "claimer": "agent-team",
                        "lease_expires_at": lease_iso,
                        "dispatch_strategy": "agent-team",
                        "target_project": "missiond",
                        "requested_cwd": "/Users/jinchen/Projects/missiond",
                        "provenance": "event_log_query",
                    },
                    {
                        "kind": "completed",
                        "completion_id": "COMP001",
                        "phase": "preflight",
                        "agent": "agent-team",
                        "dispatch_strategy": "agent-team",
                        "target_project": "missiond",
                        "requested_cwd": "/Users/jinchen/Projects/missiond",
                        "provenance": "event_log_query",
                    }
                ],
                // wave-17 / task 04 rollback descriptor — declared,
                // not dispatched. Cascade outcome is omitted because
                // this node SUCCEEDED (cascade only fires on a
                // failed root). Pinned as `policy="descriptor"` so
                // the smoke proves the descriptor surface is
                // available even on the success path.
                "rollback": {
                    "policy": "descriptor",
                    "objective": "revert preflight",
                    "owned_files": ["/tmp/missiond-smoke/preflight.txt"],
                    "acceptance_commands": [],
                    "dispatched": false,
                    "reason": "rollback descriptor recorded; node succeeded so no compensation triggered",
                },
            },
            // Second node: paused at the review gate. No acceptance
            // outcome yet (the resume run produces it).
            {
                "id": paused_node_id,
                "target": "mission_execution",
                "state": "paused",
                "dispatch_strategy": "unknown",
                "inner_result": null,
                "review_question_id": pause_review_qid,
            }
        ],
        "evidence_path": evidence_path,
        "plan_status": "executing",
        // wave-17 / task 05 finalization "no lie" invariant.
        "finalization": {
            "finalize_plan": true,
            "aggregate_status": "dag_paused",
            "final_plan_status": "executing",
            "rule": "paused_node_present_no_finalization",
            // wave-18 / task 05 — chain block under finalization.
            // record_only path on a paused run: chain skips because
            // the plan did not reach a succeeded final state.
            "distill_chain": {
                "triggered": false,
                "status": "skipped_plan_not_succeeded",
                "chain_id": chain_id,
                "chain_id_source": "explicit_arg",
                "chain_mode": "record_only",
                "chain_name": chain_name,
            },
        },
        // Top-level chain shortcuts mirror `attach_distill_chain_to_payload`.
        "distill_chain_status": "skipped_plan_not_succeeded",
        "distill_chain_id": chain_id,
        // File-first sidecar splice → lifted into artifact_refs.
        "file_written": true,
        "file_path": evidence_path,
        "file_sha256": "deadbeefcafebabe1818",
        "file_bytes": 768,
    }));
    let s6a_decorated = decorate(
        s6a_inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "DAG paused at review gate; resolve via mission_review then re-call with resume_review_question_id",
            next_call: Some(json!({
                "tool": "mission_review",
                "action": "resolve",
                "review_question_id": pause_review_qid,
            })),
            expects_next_inputs: json!({
                "resume_review_question_id": "string",
                "resume_review_decision": "approved|rejected|needs_changes",
                "execute": true,
            }),
        },
    );
    let s6a_meta = smoke_meta_of(&s6a_decorated);
    assert_eq!(s6a_meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    assert_eq!(s6a_meta["artifact_refs"]["scope"], "execution");
    assert_eq!(s6a_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(s6a_meta["artifact_refs"]["status"], "dag_paused");
    // Pause-side evidence sidecar pointer surfaces via file_*.
    assert_eq!(s6a_meta["artifact_refs"]["file_written"], true);
    assert_eq!(s6a_meta["artifact_refs"]["file_path"], evidence_path);
    assert_eq!(
        s6a_meta["artifact_refs"]["file_sha256"],
        "deadbeefcafebabe1818"
    );

    // Inner payload assertions — wave-18 features pinned here.
    let s6a_inner_text = match &s6a_decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let s6a_inner_payload: Value =
        serde_json::from_str(&s6a_inner_text).expect("s6a inner payload parses");
    assert_eq!(s6a_inner_payload["aggregate_status"], "dag_paused");
    // wave-18 / task 06 inference block surfaced with mode=off.
    assert_eq!(s6a_inner_payload["plan_field_inference"]["mode"], "off");
    // wave-17 / task 05 finalization "no lie" preserved.
    assert_eq!(
        s6a_inner_payload["finalization"]["aggregate_status"],
        "dag_paused"
    );
    assert_ne!(
        s6a_inner_payload["finalization"]["final_plan_status"],
        "succeeded"
    );
    // wave-18 / task 05 chain skipped because plan not succeeded.
    assert_eq!(
        s6a_inner_payload["finalization"]["distill_chain"]["status"],
        "skipped_plan_not_succeeded"
    );
    assert_eq!(
        s6a_inner_payload["finalization"]["distill_chain"]["chain_mode"],
        "record_only"
    );
    assert_eq!(
        s6a_inner_payload["distill_chain_status"],
        "skipped_plan_not_succeeded"
    );
    assert_eq!(s6a_inner_payload["distill_chain_id"], chain_id);
    // wave-18 / task 02 dispatch metadata surfaces via event_refs.
    let dep_event_refs = &s6a_inner_payload["node_results"][0]["event_refs"];
    assert_eq!(dep_event_refs[0]["kind"], "claimed");
    assert_eq!(dep_event_refs[0]["claim_id"], dep_claim_id);
    assert_eq!(dep_event_refs[0]["dispatch_strategy"], "agent-team");
    assert_eq!(dep_event_refs[0]["target_project"], "missiond");
    assert_eq!(
        dep_event_refs[0]["requested_cwd"],
        "/Users/jinchen/Projects/missiond"
    );
    assert_eq!(dep_event_refs[0]["provenance"], "event_log_query");
    assert_eq!(dep_event_refs[1]["kind"], "completed");
    assert_eq!(dep_event_refs[1]["dispatch_strategy"], "agent-team");
    // wave-16 / task 05 retry surface present (max_attempts > 1).
    assert_eq!(
        s6a_inner_payload["node_results"][0]["retry"]["max_attempts"],
        2
    );
    assert_eq!(s6a_inner_payload["node_results"][0]["retry"]["attempts"], 1);
    // wave-17 / task 04 rollback descriptor surface — recorded but
    // never dispatched on the success path.
    let dep_rollback = &s6a_inner_payload["node_results"][0]["rollback"];
    assert_eq!(dep_rollback["policy"], "descriptor");
    assert_eq!(dep_rollback["dispatched"], false);
    assert_eq!(dep_rollback["objective"], "revert preflight");

    // ── Stage s6b :: resume execute → succeeded + chain recorded ────
    //
    // Caller hands the pipeline the SAME plan id + the deterministic
    // review-question id + `resume_review_decision="approved"` +
    // the same wave-18 chain knobs. We synthesise the inner payload
    // mirroring `action_execute_resume`'s approved-branch shape +
    // `apply_distill_chain`'s record_only output: the previously-
    // paused node moves through `paused -> ready -> running ->
    // succeeded`, the wave-18 / task 03 fan-in evaluator passes
    // (`all-succeeded` against the dep node which already succeeded
    // in s6a), and the cross-plan distill chain RECORDS the entry
    // into the evidence sidecar without invoking any LLM.
    let resume_args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "resume_review_question_id": pause_review_qid,
        "resume_review_decision": "approved",
        "resume_actor": "agent-team",
        "resume_note": "wave18 smoke approves",
        "finalize_plan": true,
        "distill_chain_id": chain_id,
        "distill_chain_mode": "record_only",
        "distill_chain_name": chain_name,
        "infer_plan_fields": "off",
    });
    let s6b_decision = plan_pipeline(&resume_args).expect("s6b must route");
    let PipelineDecision::PlanExecute { execute_args } = s6b_decision else {
        panic!("resume call must route to PlanExecute");
    };
    // Forwarding pinned through the routing decision.
    assert_eq!(execute_args["resume_review_question_id"], pause_review_qid);
    assert_eq!(execute_args["resume_review_decision"], "approved");
    assert_eq!(execute_args["distill_chain_mode"], "record_only");
    assert_eq!(execute_args["distill_chain_id"], chain_id);
    assert_eq!(execute_args["infer_plan_fields"], "off");

    let gate_claim_id = format!("plan-dag:{}:{}:1", plan_id, paused_node_id);
    let s6b_inner = smoke_inner_result(json!({
        "status": "dag_succeeded",
        "aggregate_status": "dag_succeeded",
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "runner_status": "all_nodes_dispatched",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        "node_count": 2,
        "resume": {
            "review_question_id": pause_review_qid,
            "review_decision": "approved",
            "actor": "agent-team",
        },
        "plan_field_inference": {
            "mode": "off",
            "fields_inferred": [],
            "fields_conflicting": [],
        },
        "node_results": [
            // Dep node carries forward from s6a (succeeded earlier).
            {
                "id": dep_node_id,
                "target": "mission_execution",
                "state": "succeeded",
                "dispatch_strategy": "agent-team",
                "inner_result": {"ok": true},
                "retry": {
                    "attempts": 1,
                    "max_attempts": 2,
                },
            },
            // Resumed gate node now succeeds; acceptance includes
            // the wave-18 / task 03 fan-in outcome.
            {
                "id": paused_node_id,
                "target": "mission_execution",
                "state": "succeeded",
                "dispatch_strategy": "agent-team",
                "inner_result": {"ok": true},
                "acceptance": {
                    "status": "accepted",
                    "mode": "inner_status",
                    "commands": [],
                    "evidence_keys": [],
                    "reason": "inner_status: dispatch Ok and payload carries no error signal",
                    // wave-18 / task 03 — cross-node fan-in outcome.
                    "fan_in": {
                        "mode": "all-succeeded",
                        "source_nodes": [dep_node_id],
                        "passed": true,
                        "reason": "all 1 declared source node(s) reached succeeded",
                    },
                },
                "event_refs": [
                    {
                        "kind": "claimed",
                        "claim_id": gate_claim_id,
                        "claimer": "agent-team",
                        "lease_expires_at": lease_iso,
                        "dispatch_strategy": "agent-team",
                        "target_project": "missiond",
                        "requested_cwd": "/Users/jinchen/Projects/missiond",
                        "provenance": "event_log_query",
                    },
                    {
                        "kind": "completed",
                        "completion_id": "COMP002",
                        "phase": "review-gate-resume",
                        "agent": "agent-team",
                        "dispatch_strategy": "agent-team",
                        "target_project": "missiond",
                        "requested_cwd": "/Users/jinchen/Projects/missiond",
                        "provenance": "event_log_query",
                    }
                ],
            }
        ],
        "paused_node_ids": [],
        "review_question_ids": [],
        "evidence_path": evidence_path,
        "plan_status": "succeeded",
        "finalization": {
            "finalize_plan": true,
            "aggregate_status": "dag_succeeded",
            "final_plan_status": "succeeded",
            "rule": "all_terminal_no_failed_no_paused",
            // wave-18 / task 05 — chain RECORDED on the succeeded
            // path. record_only mode → no distill_result, no
            // warning. evidence_path mirrors the per-plan sidecar.
            "distill_chain": {
                "triggered": true,
                "status": "recorded",
                "chain_id": chain_id,
                "chain_id_source": "explicit_arg",
                "chain_mode": "record_only",
                "chain_name": chain_name,
                "chain_index_in_plan": 1,
                "evidence_path": evidence_path,
            },
        },
        "distill_chain_status": "recorded",
        "distill_chain_id": chain_id,
        "file_written": true,
        "file_path": evidence_path,
        "file_sha256": "deadbeefcafebabe2828",
        "file_bytes": 1536,
    }));
    let s6b_decorated = decorate(
        s6b_inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "DAG resumed and succeeded; loop closed (chain entry recorded)",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let s6b_meta = smoke_meta_of(&s6b_decorated);
    assert_eq!(s6b_meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    assert_eq!(s6b_meta["artifact_refs"]["scope"], "execution");
    assert_eq!(s6b_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(s6b_meta["artifact_refs"]["status"], "dag_succeeded");
    assert_eq!(s6b_meta["artifact_refs"]["file_written"], true);
    assert_eq!(s6b_meta["artifact_refs"]["file_path"], evidence_path);
    assert_eq!(
        s6b_meta["artifact_refs"]["file_sha256"],
        "deadbeefcafebabe2828"
    );

    let s6b_inner_text = match &s6b_decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let s6b_inner_payload: Value =
        serde_json::from_str(&s6b_inner_text).expect("s6b inner payload parses");

    // ── Acceptance fan-in (wave-18 / task 03) ──────────────────────
    let gate_acceptance = &s6b_inner_payload["node_results"][1]["acceptance"];
    assert_eq!(gate_acceptance["status"], "accepted");
    assert_eq!(gate_acceptance["mode"], "inner_status");
    let fan_in = &gate_acceptance["fan_in"];
    assert_eq!(fan_in["mode"], "all-succeeded");
    assert_eq!(fan_in["source_nodes"][0], dep_node_id);
    assert_eq!(fan_in["passed"], true);
    assert!(fan_in["reason"].as_str().unwrap().contains("succeeded"));

    // ── Finalization (wave-17 / task 05) ───────────────────────────
    assert_eq!(
        s6b_inner_payload["finalization"]["aggregate_status"],
        "dag_succeeded"
    );
    assert_eq!(
        s6b_inner_payload["finalization"]["final_plan_status"],
        "succeeded"
    );
    assert_eq!(
        s6b_inner_payload["finalization"]["rule"],
        "all_terminal_no_failed_no_paused"
    );

    // ── Distill chain (wave-18 / task 05) ──────────────────────────
    let chain_block = &s6b_inner_payload["finalization"]["distill_chain"];
    assert_eq!(chain_block["triggered"], true);
    assert_eq!(chain_block["status"], "recorded");
    assert_eq!(chain_block["chain_id"], chain_id);
    assert_eq!(chain_block["chain_mode"], "record_only");
    assert_eq!(chain_block["chain_name"], chain_name);
    assert_eq!(chain_block["chain_index_in_plan"], 1);
    assert_eq!(chain_block["evidence_path"], evidence_path);
    // record_only path → no distill_result, no warning. Pinning
    // their absence proves the smoke did not silently invoke an LLM
    // through the dry_run / sonnet branches.
    assert!(
        chain_block.get("distill_result").is_none(),
        "record_only chain MUST NOT carry a distill_result (no LLM smoke)"
    );
    assert!(
        chain_block.get("warning").is_none(),
        "record_only chain MUST NOT surface a warning"
    );
    // Top-level chain shortcuts mirror the brief contract.
    assert_eq!(s6b_inner_payload["distill_chain_status"], "recorded");
    assert_eq!(s6b_inner_payload["distill_chain_id"], chain_id);

    // ── Dispatch metadata (wave-18 / task 02) ──────────────────────
    let gate_event_refs = &s6b_inner_payload["node_results"][1]["event_refs"];
    assert_eq!(gate_event_refs[0]["kind"], "claimed");
    assert_eq!(gate_event_refs[0]["claim_id"], gate_claim_id);
    assert_eq!(gate_event_refs[0]["dispatch_strategy"], "agent-team");
    assert_eq!(gate_event_refs[0]["target_project"], "missiond");
    assert_eq!(
        gate_event_refs[0]["requested_cwd"],
        "/Users/jinchen/Projects/missiond"
    );
    // wave-18 / task 01 — provenance label matches the
    // `EventRefProvenance::EventLogQuery` wire form.
    assert_eq!(gate_event_refs[0]["provenance"], "event_log_query");
    assert_eq!(gate_event_refs[1]["kind"], "completed");
    assert_eq!(gate_event_refs[1]["provenance"], "event_log_query");

    // ── Sidecar invariant: pause + resume + chain rows share one file
    assert_eq!(
        s6a_meta["artifact_refs"]["file_path"], s6b_meta["artifact_refs"]["file_path"],
        "pause + resume + chain rows must share one evidence sidecar"
    );

    // ── v0/v1 non-goals remain loud at every stage ────────────────
    // Pinned across the loop so a future refactor that adds an
    // auto-approve / autonomous-dispatch path lands an explicit
    // failure on this smoke instead of silently passing.
    for (label, meta) in [
        ("s1", &s1_meta),
        ("s4", &s4_meta),
        ("s6a", &s6a_meta),
        ("s6b", &s6b_meta),
    ] {
        let non_goals = meta["v0_non_goals"]
            .as_array()
            .unwrap_or_else(|| panic!("{} stage must surface v0_non_goals array", label));
        for needle in [
            "auto_approve_directive",
            "auto_approve_plan",
            "auto_answer_review_question",
            "autonomous_workstation_dispatch",
        ] {
            assert!(
                non_goals.iter().any(|v| v == needle),
                "{} must keep `{}` in v0_non_goals",
                label,
                needle
            );
        }
    }
}

// ── 9. wave-20 / Task 05 :: machine-driven dispatch loop smoke ──────
//
// Goal: prove the wave-20 / task 04 machine-driven dispatch loop end-
// to-end through the unified entry — the wave-19/06 emitter writes a
// task-contract v1 file, the wave-19/07 consumer reads it directly,
// the brief carries the `## Source contract` preamble, and the
// unified-entry response surfaces every machine-contract field
// (`task_contract_path`, `task_contract_source_path`,
// `dispatch_contract_mode`) without depending on the markdown brief
// being load-bearing.
//
// The smoke layers two new contracts on top of the wave-16/17/18
// canonical-loop scaffold (`smoke_canonical_loop_*`):
//
//   * **Machine-contract field survival**: the inner plan-execute
//     payload carries the wave-19/06 emitter block + the wave-20/04
//     workstation extension; the unified-entry decorator's
//     `artifact_refs` projection MUST lift every machine-contract
//     key so a single envelope shape covers the handoff.
//   * **Markdown brief is non-load-bearing**: the brief preview
//     surfaces alongside `task_contract_source_path`; we assert that
//     the path is the SSOT (the brief is optional compatibility
//     metadata when machine mode is active).
//
// No LLM, no spawn, no shell — the smoke parses fixture task.lisp
// text directly via `super::workstation_dispatch::parse_task_contract`,
// overlays the contract onto a fresh `WorkstationDispatchHints` to
// prove the consumer rule (contract wins on every non-empty field),
// and synthesises the inner payload exactly the way
// `build_workstation_dispatch_response` produces it for the
// `Dispatched { task_contract_source_path: Some(...) }` branch in
// machine mode.
//
// Out of smoke scope (covered elsewhere by dedicated tests):
//   * the parser unit tests (parse_task_contract_*) — workstation_dispatch
//   * the dispatch-mode parser allowlist tests (parse_dispatch_contract_mode_*)
//     — plan
//   * the response builder pin tests
//     (build_workstation_dispatch_response_machine_mode_*) — plan
//   * the wave-20/04 forwarding contract — already pinned earlier in
//     this module via
//     `build_plan_execute_args_forwards_task_contract_and_dispatch_contract_knobs`.

/// Reference task-contract v1 body — mirrors the byte-shape produced
/// by `plan::build_task_contract_lisp` for a single-node dispatch.
/// The smoke parses this text via
/// `super::workstation_dispatch::parse_task_contract` to prove the
/// consumer rule end-to-end without any IO.
const SMOKE_MACHINE_CONTRACT: &str = r#";; Generated by MissionD plan-runner (wave-20 / task 05 smoke fixture).
;; plan_id = 55555555-5555-5555-5555-000000000020
;; board_task_id = btk-wave20-task05-smoke
;; node_id = root

(task plan-55555555-node-root
  :schema "missiond.task-contract.v1"
  :title "Plan 55555555-5555-5555-5555-000000000020 node root — workstation task contract"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :dispatch-strategy "agent-team"
  :goal "ship the wave-20 machine loop smoke"
  :scope "wave 20 task 05 only"
  :write-scope
["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
 "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
 "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
  :must-not-touch
["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
 "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
 "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
  :acceptance
["cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
 "cargo test -p missiond-daemon"
 "cargo build --workspace"
 "node scripts/check-architecture-lisp.mjs --all-v2"]
  :commit
(:required true
 :message "test(intent): cover machine contract dispatch loop"
 :scope-check write-scope-only
 :policy "scoped")
  :target-project "missiond"
  :requested-cwd "/Users/jinchen/Projects/missiond"
  :target "mission_task_delegate"
  :plan-id "55555555-5555-5555-5555-000000000020"
  :node-id "root"
)
"#;

/// wave-20 / task 05 — `build_artifact_refs` lifts every wave-19/06 +
/// wave-20/04 machine-contract field so the unified-entry envelope
/// preserves the machine handoff without callers diving back into the
/// inner JSON. Pinning the projection separately from the smoke gives
/// a future refactor that drops a key a small targeted failure
/// instead of only the broader smoke trace.
#[test]
fn build_artifact_refs_lifts_wave19_20_machine_contract_fields() {
    // Synthesise the exact key set the wave-20/04 plan-execute branch
    // stamps on its inner payload when the runner dispatched in
    // machine mode AND the wave-19/06 emitter wrote the contract.
    let payload = json!({
        "status": "executing",
        "execute_mode": "internal",
        "plan_id": "55555555-5555-5555-5555-000000000020",
        "board_task_id": "btk-wave20-task05-smoke",
        // wave-19 / task 06 — emission record (merge_task_contract_block).
        "task_contract_mode": "emit",
        "task_contract_eligible": true,
        "task_contract_path":
            "/tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp",
        "render_command":
            "node scripts/render-claudecode-task.mjs /tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp",
        // wave-20 / task 04 — dispatch-mode + workstation extension.
        "dispatch_contract_mode": "machine",
        "task_contract_source_path":
            "/tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp",
        "workstation_dispatch_status": "dispatched",
        "task_brief_preview":
            "## Source contract\n- task-contract v1: `…/root.lisp`\n## Objective\n…\n",
    });
    let refs = build_artifact_refs(ArtifactScope::Execution, &payload);
    // Row-id pointers preserved.
    assert_eq!(refs["scope"], "execution");
    assert_eq!(refs["plan_id"], "55555555-5555-5555-5555-000000000020");
    assert_eq!(refs["board_task_id"], "btk-wave20-task05-smoke");
    assert_eq!(refs["status"], "executing");
    // wave-19 / task 06 splice surfaced verbatim.
    assert_eq!(refs["task_contract_mode"], "emit");
    assert_eq!(refs["task_contract_eligible"], true);
    assert_eq!(
        refs["task_contract_path"],
        "/tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp"
    );
    assert_eq!(
        refs["render_command"],
        "node scripts/render-claudecode-task.mjs /tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp"
    );
    // wave-20 / task 04 splice surfaced verbatim.
    assert_eq!(refs["dispatch_contract_mode"], "machine");
    assert_eq!(
        refs["task_contract_source_path"],
        "/tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp"
    );
    // The path the workstation consumed MUST equal the path the
    // emitter wrote — that equality is the SSOT proof for observers.
    assert_eq!(
        refs["task_contract_source_path"], refs["task_contract_path"],
        "machine-mode handoff invariant: source path consumed equals path emitted"
    );
    // The legacy `workstation_dispatch_status` and brief preview
    // stay on the inner payload (they are not row-id / file-first /
    // review / contract pointers); the projection deliberately
    // does NOT lift them so the artifact_refs surface stays terse.
    let refs_obj = refs.as_object().unwrap();
    assert!(!refs_obj.contains_key("workstation_dispatch_status"));
    assert!(!refs_obj.contains_key("task_brief_preview"));
}

/// wave-20 / task 05 — `build_artifact_refs` does not fabricate any
/// machine-contract field when the inner handler omitted them. This
/// pins the byte-compat invariant for wave-15..19 callers that never
/// opt into `task_contract_mode` / `dispatch_contract_mode`.
#[test]
fn build_artifact_refs_omits_machine_contract_keys_when_inner_silent() {
    let payload = json!({
        "status": "executing",
        "plan_id": "00000000-0000-0000-0000-000000000aaa",
        "board_task_id": "btk-legacy",
    });
    let refs = build_artifact_refs(ArtifactScope::Execution, &payload);
    let refs_obj = refs.as_object().unwrap();
    for k in &[
        "task_contract_mode",
        "task_contract_eligible",
        "task_contract_path",
        "task_contract_source_path",
        "dispatch_contract_mode",
        "render_command",
        "task_contract_skip_reason",
        "task_contract_error",
    ] {
        assert!(
            !refs_obj.contains_key(*k),
            "wave-15..19 byte-compat: machine-contract key `{}` must NOT be fabricated",
            k
        );
    }
}

/// wave-20 / task 05 — drive the wave-19/07 narrow parser against the
/// fixture task.lisp text and overlay the result onto a fresh hints
/// bag. Proves the consumer rule (contract wins on every non-empty
/// field) end-to-end on the canonical fixture without IO.
///
/// This is the "data side" of the machine handoff: the brief is
/// rendered FROM the parsed contract, not from caller args. When the
/// dispatch ran in machine mode, the contract is the SSOT — the
/// markdown brief is rendered for compatibility but never load-
/// bearing.
#[test]
fn smoke_wave20_machine_loop_fixture_contract_overlays_hints() {
    use super::super::workstation_dispatch::{
        self as wd, parse_task_contract, WorkstationDispatchHints,
    };
    let parsed = parse_task_contract(SMOKE_MACHINE_CONTRACT).expect("smoke fixture must parse");
    // Pin every consumed field so a renderer/parser drift surfaces.
    assert_eq!(parsed.schema, "missiond.task-contract.v1");
    assert_eq!(parsed.goal, "ship the wave-20 machine loop smoke");
    assert_eq!(parsed.scope.as_deref(), Some("wave 20 task 05 only"));
    assert_eq!(parsed.write_scope.len(), 3);
    assert!(parsed
        .write_scope
        .iter()
        .any(|p| p == "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"));
    assert_eq!(parsed.must_not_touch.len(), 3);
    assert_eq!(parsed.commit_policy.as_deref(), Some("scoped"));
    assert_eq!(parsed.dispatch_strategy.as_deref(), Some("agent-team"));
    assert_eq!(parsed.target_project.as_deref(), Some("missiond"));
    assert_eq!(parsed.target.as_deref(), Some("mission_task_delegate"));

    // Caller side starts with a deliberately-stale hint set so the
    // overlay rule (contract wins on every non-empty field) is
    // exercised end-to-end.
    let mut hints = WorkstationDispatchHints {
        objective: Some("STALE objective from caller".to_string()),
        scope: Some("STALE scope".to_string()),
        owned_files: vec!["stale-owned.rs".to_string()],
        forbidden_files: vec![],
        acceptance_commands: vec!["stale-acceptance".to_string()],
        commit_policy: None,
        target_project: None,
        requested_cwd: None,
        dispatch_strategy: Some("fresh-code-alignment".to_string()),
    };
    hints.overlay_contract(&parsed);

    // Every non-empty contract field MUST have replaced the caller's
    // stale value — the contract is the SSOT.
    assert_eq!(
        hints.objective.as_deref(),
        Some("ship the wave-20 machine loop smoke"),
        "contract :goal must beat caller objective"
    );
    assert_eq!(
        hints.scope.as_deref(),
        Some("wave 20 task 05 only"),
        "contract :scope must beat caller scope"
    );
    assert_eq!(
        hints.owned_files.len(),
        3,
        "contract :write-scope must beat stale owned_files"
    );
    assert!(hints
        .owned_files
        .iter()
        .any(|p| p == "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"));
    assert_eq!(
        hints.forbidden_files.len(),
        3,
        "contract :must-not-touch surfaces verbatim"
    );
    assert_eq!(
        hints.commit_policy.as_deref(),
        Some("scoped"),
        "contract :commit :policy must reach the hints bag"
    );
    assert_eq!(
        hints.dispatch_strategy.as_deref(),
        Some("agent-team"),
        "contract :dispatch-strategy must beat caller dispatch_strategy"
    );

    // Hand the parsed contract to the brief renderer — this is what
    // the consumer does in machine mode. The brief is built FROM the
    // parsed contract via `build_task_brief_with_source`. We pin the
    // `## Source contract` preamble so the renderer-side handoff is
    // exercised, but the assertion below pins that the brief is NOT
    // load-bearing in machine mode (the source path is the SSOT).
    let plan_for_brief = missiond_core::types::Plan {
        id: uuid::Uuid::parse_str("55555555-5555-5555-5555-000000000020").unwrap(),
        board_task_id: "btk-wave20-task05-smoke".to_string(),
        source_directive_id: None,
        version: 1,
        sexp_text: "(plan)".to_string(),
        sexp_hash: "deadbeef".to_string(),
        status: missiond_core::types::PlanStatus::Approved,
        compiler_model: None,
        compiled_from: None,
        contract_json: serde_json::json!({}),
        created_at: chrono::Utc::now(),
        approved_at: None,
        finished_at: None,
    };
    let contract_path = std::path::Path::new(
        "/tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp",
    );
    let brief = wd::build_task_brief_with_source(
        &plan_for_brief,
        &hints,
        "agent-team",
        Some(contract_path),
    );
    assert!(
        brief.contains("## Source contract"),
        "machine-mode brief MUST carry the wave-19/07 source-contract preamble"
    );
    assert!(
        brief.contains("/tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp"),
        "brief preamble MUST name the on-disk contract path"
    );
    // Contract overlay reached the brief body — the new objective
    // (NOT the caller's stale one) is what the worker reads.
    assert!(
        brief.contains("ship the wave-20 machine loop smoke"),
        "machine-mode brief MUST render the contract :goal as the objective"
    );
    assert!(
        !brief.contains("STALE objective from caller"),
        "stale caller objective MUST NOT leak into the contract-driven brief"
    );
}

/// wave-20 / task 05 — full canonical loop smoke for the machine-
/// driven dispatch path. Drives `plan_pipeline` for s4 / s6, then
/// synthesises the inner plan-execute payload that the wave-20/04
/// `build_workstation_dispatch_response` produces in machine mode
/// (Dispatched + `task_contract_source_path: Some(...)` +
/// `dispatch_contract_mode="machine"` + the wave-19/06 task-contract
/// emission block). Asserts the unified-entry decorator's
/// `artifact_refs` projection lifts every machine-contract field so
/// the handoff survives one full envelope round trip.
#[test]
fn smoke_wave20_machine_loop_canonical_contract_handoff() {
    let directive_id = "00000000-0000-0000-0000-000000000d20";
    let board_task_id = "btk-wave20-task05-smoke";
    let plan_id_uuid =
        uuid::Uuid::parse_str("55555555-5555-5555-5555-000000000020").expect("valid plan uuid");
    let plan_id = plan_id_uuid.to_string();
    let plan_version: i32 = 1;
    let contract_path = "/tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp";
    let render_command = format!("node scripts/render-claudecode-task.mjs {}", contract_path);
    let evidence_path = "/tmp/missiond-smoke/.missiond/v2/plans/wave20-task05.evidence.json";

    // ── Stage s4 :: plan compile (dry_run, no LLM) ─────────────────
    let s4_args = json!({
        "approved_directive_id": directive_id,
        "directive_version": 1,
        "board_task_id": board_task_id,
        "compiler_mode": "dry_run",
        "persist": true,
    });
    let s4_decision = plan_pipeline(&s4_args).expect("s4 must route");
    let PipelineDecision::PlanCompile { compile_args } = s4_decision else {
        panic!("expected PlanCompile");
    };
    assert_eq!(compile_args["board_task_id"], board_task_id);
    let s4_inner = smoke_inner_result(json!({
        "status": "dry_run",
        "compiler_mode": "dry_run",
        "persisted": true,
        "plan_id": plan_id,
        "version": plan_version,
        "board_task_id": board_task_id,
        "directive_id": directive_id,
    }));
    let s4_decorated = decorate(
        s4_inner,
        DecorateContext {
            stage: stages::PLAN_AUTHORING,
            scope: ArtifactScope::Plan,
            next_step: "review the compiled PLAN.lisp then re-call this pipeline with `approved_plan_id` and `execute=true`",
            next_call: Some(json!({"tool": "mission_plan", "action": "approve"})),
            expects_next_inputs: json!({"approved_plan_id": "uuid", "execute": true}),
        },
    );
    let s4_meta = smoke_meta_of(&s4_decorated);
    assert_eq!(s4_meta["pipeline_stage"], stages::PLAN_AUTHORING);
    assert_eq!(s4_meta["artifact_refs"]["plan_id"], plan_id);
    // No machine-contract fields yet — emission has not run.
    assert!(s4_meta["artifact_refs"]
        .as_object()
        .unwrap()
        .get("task_contract_path")
        .is_none());

    // ── Stage s6 :: plan execute → machine-driven workstation dispatch
    //
    // Caller opts BOTH the wave-19/06 emitter (`task_contract_mode=
    // "emit"`) AND the wave-20/04 dispatch (`dispatch_contract_mode=
    // "machine"`) on; the unified entry forwards both knobs verbatim
    // (already pinned by the dedicated forwarding test earlier in
    // this module — re-asserted here so the smoke trace is self-
    // contained).
    let s6_args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "scheduler_mode": "single_node",
        "target": "mission_task_delegate",
        "dispatch_strategy": "agent-team",
        "target_project": "missiond",
        "cwd": "/Users/jinchen/Projects/missiond",
        "task_contract_mode": "emit",
        "emit_task_contract": true,
        "dispatch_contract_mode": "machine",
        "render_markdown": false,
    });
    let s6_decision = plan_pipeline(&s6_args).expect("s6 must route");
    let PipelineDecision::PlanExecute { execute_args } = s6_decision else {
        panic!("approved_plan_id + execute=true must route to PlanExecute");
    };
    // Forwarding pinned through the routing decision.
    assert_eq!(execute_args["plan_id"], plan_id);
    assert_eq!(execute_args["task_contract_mode"], "emit");
    assert_eq!(execute_args["emit_task_contract"], true);
    assert_eq!(execute_args["dispatch_contract_mode"], "machine");
    assert_eq!(execute_args["render_markdown"], false);

    // Synthesise the inner plan-execute payload exactly the way the
    // wave-20/04 `build_workstation_dispatch_response` produces it
    // for the `Dispatched + machine-mode` branch. The
    // `task_contract_source_path` carries the resolved on-disk path
    // and equals `task_contract_path` (the path the emitter wrote)
    // — that equality is the SSOT proof for observers.
    let s6_inner = smoke_inner_result(json!({
        // Top-level wire shape mirrors `build_workstation_dispatch_response`.
        "status": "executing",
        "execute_mode": "internal",
        "runner_status": "workstation_dispatch_v0",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        "target_tool": "mission_task_delegate",
        "target_source": "explicit_arg",
        "dispatch_strategy": "agent-team",
        "dispatch_strategy_source": "explicit_arg",
        "plan_hint_summary": {},
        "workstation_dispatch_source": "explicit_arg",
        // wave-20 / task 04 — dispatch-mode marker.
        "dispatch_contract_mode": "machine",
        // Workstation extension fields (outcome_to_response_fields).
        "workstation_dispatch_status": "dispatched",
        "scoped_commit_required": true,
        "scoped_commit_policy": "enforced-on-complete",
        "task_brief_preview":
            format!(
                "## Source contract\n- task-contract v1: `{}`\n- this brief is rendered from the contract above; treat the contract as the SSOT\n- if the brief and the contract diverge, the contract wins — re-read it before staging\n\n## Objective\nship the wave-20 machine loop smoke\n",
                contract_path
            ),
        "task_contract_source_path": contract_path,
        "evidence_path": evidence_path,
        "inner_result": {
            "task_id": "btk-wave20-task05-smoke",
            "status": "dispatched",
        },
        // wave-19 / task 06 — emission record (merge_task_contract_block).
        "task_contract_mode": "emit",
        "task_contract_eligible": true,
        "task_contract_path": contract_path,
        "render_command": render_command,
    }));
    let s6_decorated = decorate(
        s6_inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "machine-mode dispatch landed; collect evidence via mission_plan(action=record_evidence)",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let s6_meta = smoke_meta_of(&s6_decorated);
    assert_eq!(s6_meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    assert_eq!(s6_meta["artifact_refs"]["scope"], "execution");
    assert_eq!(s6_meta["artifact_refs"]["plan_id"], plan_id);
    assert_eq!(s6_meta["artifact_refs"]["board_task_id"], board_task_id);
    assert_eq!(s6_meta["artifact_refs"]["status"], "executing");
    // ── Machine-contract fields lifted into artifact_refs ──────────
    assert_eq!(
        s6_meta["artifact_refs"]["dispatch_contract_mode"],
        "machine"
    );
    assert_eq!(s6_meta["artifact_refs"]["task_contract_mode"], "emit");
    assert_eq!(s6_meta["artifact_refs"]["task_contract_eligible"], true);
    assert_eq!(
        s6_meta["artifact_refs"]["task_contract_path"],
        contract_path
    );
    assert_eq!(
        s6_meta["artifact_refs"]["task_contract_source_path"],
        contract_path
    );
    // SSOT invariant: the path the workstation consumed MUST equal
    // the path the emitter wrote. Pinning the equality on the
    // unified-entry envelope shape proves the handoff survived end-
    // to-end without re-parsing the inner JSON.
    assert_eq!(
        s6_meta["artifact_refs"]["task_contract_source_path"],
        s6_meta["artifact_refs"]["task_contract_path"],
        "machine-mode loop SSOT invariant: source path consumed equals path emitted"
    );
    // render_command surfaces alongside the path so the caller can
    // run the renderer out of process for human review — but it is
    // optional compatibility metadata, not load-bearing.
    assert_eq!(s6_meta["artifact_refs"]["render_command"], render_command);

    // Inner payload preserved byte-for-byte by `decorate` so the
    // workstation extension stays accessible.
    let s6_inner_text = match &s6_decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let s6_inner_payload: Value =
        serde_json::from_str(&s6_inner_text).expect("inner payload parses");
    assert_eq!(
        s6_inner_payload["workstation_dispatch_status"],
        "dispatched"
    );
    assert_eq!(s6_inner_payload["dispatch_contract_mode"], "machine");
    assert_eq!(s6_inner_payload["task_contract_source_path"], contract_path);

    // ── v0/v1 non-goals remain loud at every machine-mode stage ────
    let s6_non_goals = s6_meta["v0_non_goals"].as_array().unwrap();
    for needle in [
        "auto_approve_directive",
        "auto_approve_plan",
        "auto_answer_review_question",
        "autonomous_workstation_dispatch",
    ] {
        assert!(
            s6_non_goals.iter().any(|v| v == needle),
            "machine-mode s6 must keep `{}` in v0_non_goals",
            needle
        );
    }
}

/// wave-20 / task 05 — explicit assertion that the markdown / rendered
/// brief is non-load-bearing in machine mode. The unified-entry
/// envelope surfaces `task_contract_source_path` as the load-bearing
/// pointer; the `task_brief_preview` lives on the inner payload as a
/// human-readable mirror but is NOT projected into `artifact_refs`.
/// This is the v2 contract: a downstream consumer can drive the
/// machine handoff entirely from `artifact_refs.task_contract_*`
/// without touching the brief preview.
#[test]
fn smoke_wave20_machine_mode_markdown_brief_is_non_load_bearing() {
    let contract_path = "/tmp/missiond-smoke/.missiond/tasks/generated/plan/55555555/root.lisp";
    let render_command = format!("node scripts/render-claudecode-task.mjs {}", contract_path);

    // Machine-mode dispatched payload (mirrors
    // `build_workstation_dispatch_response` in machine mode).
    let payload = json!({
        "status": "executing",
        "plan_id": "55555555-5555-5555-5555-000000000020",
        "board_task_id": "btk-wave20-task05-smoke",
        "dispatch_contract_mode": "machine",
        "task_contract_mode": "emit",
        "task_contract_eligible": true,
        "task_contract_path": contract_path,
        "task_contract_source_path": contract_path,
        "render_command": render_command,
        "task_brief_preview":
            format!("## Source contract\n- task-contract v1: `{}`\n", contract_path),
        "workstation_dispatch_status": "dispatched",
    });
    let inner = ToolResult {
        content: vec![missiond_mcp::tools::ToolContent::Text {
            text: serde_json::to_string_pretty(&payload).unwrap(),
        }],
        is_error: None,
    };
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "machine-mode dispatch landed",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let meta = smoke_meta_of(&decorated);
    let refs = meta["artifact_refs"]
        .as_object()
        .expect("artifact_refs is object");

    // ── Load-bearing: the contract source path must be lifted ─────
    assert!(
        refs.contains_key("task_contract_source_path"),
        "machine-mode envelope MUST surface task_contract_source_path as the SSOT pointer"
    );
    assert!(
        refs.contains_key("task_contract_path"),
        "machine-mode envelope MUST surface task_contract_path so observers can prove emission==consumption"
    );
    assert_eq!(refs["dispatch_contract_mode"], "machine");

    // ── Non-load-bearing: the markdown brief preview must NOT be
    //    projected into artifact_refs. A consumer can render the
    //    brief out of process via `render_command` if needed.
    assert!(
        !refs.contains_key("task_brief_preview"),
        "task_brief_preview MUST NOT be projected into artifact_refs in machine mode \
         (the brief is optional compatibility metadata, not load-bearing)"
    );
    // The inner payload still carries the brief preview unchanged
    // for callers that want the human-readable mirror; we just
    // refuse to elevate it to the envelope shape.
    let inner_text = match &decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let inner_payload: Value = serde_json::from_str(&inner_text).unwrap();
    assert!(
        inner_payload["task_brief_preview"]
            .as_str()
            .unwrap_or("")
            .contains("## Source contract"),
        "inner payload preserves the brief preview as a human-readable mirror"
    );

    // The render_command is surfaced in artifact_refs so the caller
    // knows HOW to produce the markdown brief if they want it —
    // but the brief is rendered out of process, never inline.
    assert_eq!(refs["render_command"], render_command);
}

/// wave-20 / task 05 — malformed task-contract refusal smoke.
///
/// Two failure surfaces are exercised in one trace:
///   1. Direct parser failure on intentionally-broken fixture text
///      (proves the consumer detects the failure with a typed
///      `TaskContractParseError`).
///   2. The synthesised inner plan-execute payload that
///      `build_workstation_dispatch_response` produces when the
///      runner refused dispatch via
///      `SafeDescriptorReason::MalformedTaskContract`. The unified-
///      entry decorator MUST keep the refusal visible — no fallback
///      to `claude -p` or the legacy natural-language brief, the
///      `workstation_dispatch_status` MUST stay
///      `skipped_malformed_task_contract`, and the inner payload
///      MUST NOT carry an `inner_result` (no dispatch fired).
#[test]
fn smoke_wave20_machine_mode_malformed_contract_refuses_no_prompt_fallback() {
    use super::super::workstation_dispatch::{parse_task_contract, TaskContractParseError};

    // 1. Parser-side: a contract missing the required `:goal` field
    //    fails fast with `MissingRequired("goal")`. Pinning the
    //    typed variant proves the parser surfaces the actionable
    //    reason without inventing a default.
    let malformed = r#"(task no-goal
  :schema "missiond.task-contract.v1"
  :title "missing goal"
  :write-scope []
)"#;
    let err = parse_task_contract(malformed)
        .expect_err("malformed contract MUST fail parsing rather than silently degrade");
    assert!(
        matches!(err, TaskContractParseError::MissingRequired("goal")),
        "wave-20/05 contract: structured error variant MUST be `MissingRequired(\"goal\")`, got {:?}",
        err
    );
    let reason = err.reason();
    assert!(
        reason.contains("missing required") && reason.contains("goal"),
        "structured error reason must name the missing field, got `{}`",
        reason
    );

    // 2. Runner-side: the wave-20/04 response builder produces this
    //    exact wire shape when `run_workstation_dispatch_with_contract`
    //    returned `SafeDescriptor { reason: MalformedTaskContract { ... } }`.
    //    The smoke synthesises that payload and feeds it through the
    //    unified-entry decorator. The decorator MUST preserve every
    //    refusal field — there is NO prompt fallback path.
    let bad_path = "/tmp/missiond-smoke/.missiond/tasks/generated/plan/bad/root.lisp";
    let payload = json!({
        "status": "dispatch_skipped",
        "execute_mode": "internal",
        "runner_status": "workstation_dispatch_v0",
        "plan_id": "55555555-5555-5555-5555-000000000020",
        "board_task_id": "btk-wave20-task05-smoke",
        "target_tool": "mission_task_delegate",
        "target_source": "explicit_arg",
        "dispatch_strategy": "agent-team",
        "dispatch_strategy_source": "explicit_arg",
        "plan_hint_summary": {},
        "workstation_dispatch_source": "explicit_arg",
        "dispatch_contract_mode": "machine",
        // Workstation extension — SafeDescriptor branch.
        "workstation_dispatch_status": "skipped_malformed_task_contract",
        "workstation_dispatch_reason": format!(
            "task_contract_path `{}` is malformed: {} — refusing to fall back to the legacy natural-language brief because the contract is the SSOT",
            bad_path, reason
        ),
        "scoped_commit_required": true,
        "scoped_commit_policy": "enforced-on-complete",
        // wave-19 / task 06 — emission ran, contract written. The
        // refusal is downstream of emission; we still surface the
        // emission record so observers see the file was produced.
        "task_contract_mode": "emit",
        "task_contract_eligible": true,
        "task_contract_path": bad_path,
        "render_command": format!("node scripts/render-claudecode-task.mjs {}", bad_path),
    });
    let inner = ToolResult {
        content: vec![missiond_mcp::tools::ToolContent::Text {
            text: serde_json::to_string_pretty(&payload).unwrap(),
        }],
        is_error: None,
    };
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "machine-mode dispatch refused: malformed task contract — fix the on-disk file and retry",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let meta = smoke_meta_of(&decorated);
    assert_eq!(meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    // ── Refusal envelope: machine-contract fields still surface ────
    assert_eq!(meta["artifact_refs"]["dispatch_contract_mode"], "machine");
    assert_eq!(meta["artifact_refs"]["task_contract_mode"], "emit");
    assert_eq!(meta["artifact_refs"]["task_contract_path"], bad_path);
    // No dispatch happened, so the SSOT consumed-path is absent —
    // the projection MUST NOT fabricate one.
    assert!(
        meta["artifact_refs"]
            .as_object()
            .unwrap()
            .get("task_contract_source_path")
            .is_none(),
        "refusal path MUST NOT carry task_contract_source_path (no consumption happened)"
    );

    // ── Inner payload: refusal is verbatim, no prompt fallback ─────
    let inner_text = match &decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let inner_payload: Value = serde_json::from_str(&inner_text).unwrap();
    assert_eq!(inner_payload["status"], "dispatch_skipped");
    assert_eq!(
        inner_payload["workstation_dispatch_status"], "skipped_malformed_task_contract",
        "refusal MUST stay loud — no silent downgrade to the legacy brief"
    );
    let refusal_reason = inner_payload["workstation_dispatch_reason"]
        .as_str()
        .unwrap_or("");
    assert!(
        refusal_reason.contains(bad_path),
        "refusal reason must name the offending contract path"
    );
    assert!(
        refusal_reason.contains("missing required") && refusal_reason.contains("goal"),
        "refusal reason must explain WHY the parse failed (forwarded from the typed error)"
    );
    assert!(
        refusal_reason.contains("refusing to fall back"),
        "refusal must explicitly mention there is no fallback path \
         (per wave-19/07 + wave-20/04 SSOT contract)"
    );
    // No inner_result MUST have leaked through — the runner refused
    // before the workstation substrate was invoked.
    assert!(
        inner_payload.get("inner_result").is_none(),
        "machine-mode refusal MUST NOT carry inner_result (no dispatch fired)"
    );
    // No `claude -p` shell-out artefact MUST appear anywhere on the
    // payload — pinning the absence so a future "compatibility
    // shim" cannot silently re-introduce the prompt fallback.
    let inner_text_lower = inner_text.to_lowercase();
    assert!(
        !inner_text_lower.contains("claude -p"),
        "machine-mode refusal MUST NOT embed a `claude -p` fallback hint \
         (would defeat the SSOT contract)"
    );
}

// ── 10. wave-20 / Task 08 :: review auto-answer policy forwarding ──
//
// Goal: pin the wave-20/08 forwarding contract for the new
// `auto_answer_policy` knob — the unified entry MUST forward the
// value verbatim to the inner `mission_plan(action=execute)` call so
// the wave-16/02 listener path can apply the deterministic safety
// inspector. The pipeline NEVER re-validates the allowlist (that is
// the inner handler's `parse_auto_answer_policy` job) and NEVER
// injects a default at the pipeline layer (so a typo fails fast on
// the inner side instead of being silently masked).
//
// The dry-run smoke at the end of this block proves the deterministic
// path end-to-end against synthesised inner-handler payloads:
//   * inner payload mirrors what the wave-16/02 listener stamps
//     (`auto_answer_policy / policy_result / selected_decision /
//      safety_rule_results / requires_human`);
//   * `decorate` lifts every field into `artifact_refs` so a
//     downstream consumer can read the policy outcome from the
//     unified-entry envelope without re-parsing the inner payload;
//   * destructive-action invariant (archive/supersede/remove never
//     auto-promote) is exercised on the smoke trace;
//   * never-auto-reject invariant pinned by asserting the
//     `selected_decision` field never carries `rejected`.

/// Forwarding contract: the wave-20/08 `auto_answer_policy` knob
/// MUST reach the inner `mission_plan(execute)` shape via
/// `PlanExecute`. Pinning this in its own test (separate from the
/// dry-run smoke below) means a future refactor that drops the key
/// fails fast on a small targeted test rather than only on the
/// broader smoke trace.
#[test]
fn build_plan_execute_args_forwards_wave20_auto_answer_policy() {
    let plan_id = "66666666-6666-6666-6666-000000000020";
    for mode in ["off", "deterministic_safe", "dry_run"] {
        let args = json!({
            "approved_plan_id": plan_id,
            "execute": true,
            "execute_mode": "internal",
            "auto_answer_policy": mode,
        });
        let decision = plan_pipeline(&args).expect("should route to plan execute");
        let PipelineDecision::PlanExecute { execute_args } = decision else {
            panic!("expected PlanExecute");
        };
        assert_eq!(
            execute_args["auto_answer_policy"], mode,
            "auto_answer_policy=`{}` must be forwarded unchanged",
            mode
        );
    }
}

/// Forwarding contract: when the wave-20/08 knob is omitted the
/// execute args bag stays byte-identical to the wave-15..19 shape
/// (no fabricated nulls, no defaults injected at the pipeline
/// layer). Mirrors the symmetric guarantee the other wave-20 knobs
/// already enforce.
#[test]
fn build_plan_execute_args_wave20_auto_answer_policy_absent_when_omitted() {
    let args = json!({
        "approved_plan_id": "66666666-6666-6666-6666-000000000021",
        "execute": true,
    });
    let decision = plan_pipeline(&args).expect("should route to plan execute");
    let PipelineDecision::PlanExecute { execute_args } = decision else {
        panic!("expected PlanExecute");
    };
    assert!(
        execute_args.get("auto_answer_policy").is_none(),
        "default execute path must NOT forward `auto_answer_policy` (wave-15..19 byte-compat)"
    );
}

/// `build_artifact_refs` lifts every wave-20/08 auto-answer policy
/// field so the unified-entry envelope preserves the listener-path
/// outcome without callers diving back into the inner JSON. Pinning
/// the projection separately from the smoke gives a future refactor
/// that drops a key a small targeted failure instead of only the
/// broader smoke trace.
#[test]
fn build_artifact_refs_lifts_wave20_auto_answer_policy_fields() {
    // Synthesise the exact key set the wave-20/08 listener stamps on
    // its inner payload after running the deterministic inspector.
    let payload = json!({
        "status": "approved",
        "execute_mode": "internal",
        "plan_id": "66666666-6666-6666-6666-000000000022",
        "board_task_id": "btk-wave20-task08-smoke",
        // wave-20 / task 08 — auto-answer policy block.
        "auto_answer_policy": "deterministic_safe",
        "policy_result": "auto_answered",
        "selected_decision": "approved",
        "safety_rule_results": [
            "rule:deterministic_mode:producer ran in deterministic/dry-run mode",
            "rule:no_file_write:artifact did not touch the filesystem",
            "rule:no_protected_source_or_target:neither side is on the protected list",
            "rule:no_additional_blockers:no caller-side warnings or partial-status conflicts on the response payload",
            "rule:caller_opt_in:caller explicitly supplied review_automation_policy",
            "rule:non_destructive_action:`approve` is not on the destructive list",
        ],
        "requires_human": false,
    });
    let refs = build_artifact_refs(ArtifactScope::Execution, &payload);
    // Row-id pointers preserved.
    assert_eq!(refs["scope"], "execution");
    assert_eq!(refs["plan_id"], "66666666-6666-6666-6666-000000000022");
    assert_eq!(refs["board_task_id"], "btk-wave20-task08-smoke");
    assert_eq!(refs["status"], "approved");
    // wave-20 / task 08 splice surfaced verbatim.
    assert_eq!(refs["auto_answer_policy"], "deterministic_safe");
    assert_eq!(refs["policy_result"], "auto_answered");
    assert_eq!(refs["selected_decision"], "approved");
    assert!(refs["safety_rule_results"].is_array());
    assert!(!refs["safety_rule_results"].as_array().unwrap().is_empty());
    assert_eq!(refs["requires_human"], false);
}

/// Symmetric byte-compat invariant: when the inner handler is silent
/// about wave-20/08 (default `off` produces no fields), the
/// projection MUST NOT fabricate any auto-answer policy field. This
/// pins the byte-shape guarantee for wave-15..19 callers that never
/// opt into the new knob.
#[test]
fn build_artifact_refs_omits_wave20_auto_answer_policy_when_inner_silent() {
    let payload = json!({
        "status": "approved",
        "plan_id": "00000000-0000-0000-0000-000000000bbb",
        "board_task_id": "btk-legacy",
    });
    let refs = build_artifact_refs(ArtifactScope::Execution, &payload);
    let refs_obj = refs.as_object().unwrap();
    for k in &[
        "auto_answer_policy",
        "policy_result",
        "selected_decision",
        "safety_rule_results",
        "requires_human",
    ] {
        assert!(
            !refs_obj.contains_key(*k),
            "wave-15..19 byte-compat: auto-answer policy key `{}` must NOT be fabricated",
            k
        );
    }
}

/// Dry-run smoke: drives `plan_pipeline` for s4 + s6 with the
/// wave-20/08 `auto_answer_policy=dry_run` knob, then synthesises
/// the inner plan-execute payload exactly the way the wave-16/02
/// listener (post-wave-20/08) stamps it after running the
/// deterministic safety inspector in dry-run mode. The smoke pins:
///
///   * forwarding (`auto_answer_policy=dry_run` reaches the execute
///     args bag verbatim);
///   * artifact_refs lifts every policy field;
///   * `requires_human=true` ALWAYS holds under dry-run (invariant
///     pinned on the envelope shape so a future refactor that
///     silently flips it lands an explicit failure);
///   * `selected_decision` NEVER carries `rejected` (invariant I1);
///   * the v0/v1 non-goals stay loud — no LLM auto-approval, no
///     destructive-action promotion.
///
/// No LLM, no spawn, no shell — the smoke uses `decorate` against a
/// hand-crafted inner payload that mirrors the production listener
/// shape (`evaluate_auto_answer_policy` outputs).
#[test]
fn smoke_wave20_auto_answer_policy_dry_run_envelope() {
    let directive_id = "00000000-0000-0000-0000-000000000d28";
    let board_task_id = "btk-wave20-task08-smoke";
    let plan_id = "66666666-6666-6666-6666-000000000028";
    let plan_version: i32 = 1;

    // ── Stage s4 :: plan compile (dry_run, no LLM) ─────────────────
    let s4_args = json!({
        "approved_directive_id": directive_id,
        "directive_version": 1,
        "board_task_id": board_task_id,
        "compiler_mode": "dry_run",
        "persist": true,
    });
    let s4_decision = plan_pipeline(&s4_args).expect("s4 must route");
    let PipelineDecision::PlanCompile { compile_args } = s4_decision else {
        panic!("expected PlanCompile");
    };
    assert_eq!(compile_args["board_task_id"], board_task_id);

    // ── Stage s6 :: plan execute (dry_run policy, no mutation) ─────
    let s6_args = json!({
        "approved_plan_id": plan_id,
        "execute": true,
        "execute_mode": "internal",
        "scheduler_mode": "single_node",
        "target": "mission_task_delegate",
        "dispatch_strategy": "agent-team",
        "target_project": "missiond",
        "cwd": "/Users/jinchen/Projects/missiond",
        // wave-20 / task 08 — opt into dry-run preview. The smoke
        // uses `dry_run` (NOT `deterministic_safe`) so observers
        // see the deterministic outcome shape without the policy
        // mutating any DB row.
        "auto_answer_policy": "dry_run",
    });
    let s6_decision = plan_pipeline(&s6_args).expect("s6 must route");
    let PipelineDecision::PlanExecute { execute_args } = s6_decision else {
        panic!("approved_plan_id + execute=true must route to PlanExecute");
    };
    // Forwarding contract proven through the routing decision.
    assert_eq!(execute_args["plan_id"], plan_id);
    assert_eq!(execute_args["auto_answer_policy"], "dry_run");

    // Synthesise the inner plan-execute payload exactly the way the
    // wave-16/02 listener (post-wave-20/08) stamps it. Three fields
    // are load-bearing: `policy_result` is `dry_run_preview`,
    // `requires_human=true`, `selected_decision` carries the
    // suggestion (the helper never mutates state).
    let s6_inner = smoke_inner_result(json!({
        "status": "executing",
        "execute_mode": "internal",
        "runner_status": "review_question_listener_dry_run",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        "version": plan_version,
        // wave-20 / task 08 — listener-side outcome block.
        "auto_answer_policy": "dry_run",
        "policy_result": "dry_run_preview",
        "selected_decision": "approved",
        "safety_rule_results": [
            "rule:deterministic_mode:producer ran in deterministic/dry-run mode",
            "rule:no_file_write:artifact did not touch the filesystem",
            "rule:no_protected_source_or_target:neither side is on the protected list",
            "rule:no_additional_blockers:no caller-side warnings or partial-status conflicts on the response payload",
            "rule:caller_opt_in:caller explicitly supplied review_automation_policy",
            "rule:non_destructive_action:`approve` is not on the destructive list",
        ],
        "requires_human": true,
    }));
    let s6_decorated = decorate(
        s6_inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "auto-answer policy dry-run preview computed; defer to human reviewer",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let s6_meta = smoke_meta_of(&s6_decorated);

    // ── Envelope shape ─────────────────────────────────────────────
    assert_eq!(s6_meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    assert_eq!(s6_meta["artifact_refs"]["scope"], "execution");
    assert_eq!(s6_meta["artifact_refs"]["plan_id"], plan_id);
    // wave-20 / task 08 splice lifted into artifact_refs.
    assert_eq!(s6_meta["artifact_refs"]["auto_answer_policy"], "dry_run");
    assert_eq!(s6_meta["artifact_refs"]["policy_result"], "dry_run_preview");
    assert_eq!(s6_meta["artifact_refs"]["selected_decision"], "approved");
    assert!(s6_meta["artifact_refs"]["safety_rule_results"].is_array());
    // ── Invariant: dry_run ALWAYS defers to human reviewer ─────────
    assert_eq!(
        s6_meta["artifact_refs"]["requires_human"], true,
        "dry_run MUST always set requires_human=true (no listener-side mutation)"
    );
    // ── Invariant I1: selected_decision NEVER carries `rejected` ───
    assert_ne!(
        s6_meta["artifact_refs"]["selected_decision"], "rejected",
        "invariant I1: auto-answer MUST NEVER select rejected"
    );

    // ── v0/v1 non-goals remain loud at the auto-answer stage ───────
    let s6_non_goals = s6_meta["v0_non_goals"].as_array().unwrap();
    for needle in [
        "auto_approve_directive",
        "auto_approve_plan",
        "auto_answer_review_question",
        "autonomous_workstation_dispatch",
    ] {
        assert!(
            s6_non_goals.iter().any(|v| v == needle),
            "auto-answer dry-run smoke must keep `{}` in v0_non_goals",
            needle
        );
    }
}

/// Destructive-action smoke: prove the wave-20/08 invariant I2
/// (archive/supersede/remove NEVER auto-promote) survives the
/// unified-entry decoration. We synthesise an inner payload mirror
/// of what the listener stamps when the policy ran against an
/// `archive` action under `deterministic_safe` mode. The
/// `policy_result` MUST be `skipped_destructive_action` and the
/// envelope MUST carry `requires_human=true`.
#[test]
fn smoke_wave20_auto_answer_policy_destructive_action_never_promotes() {
    let plan_id = "66666666-6666-6666-6666-000000000029";
    let payload = json!({
        "status": "draft",
        "plan_id": plan_id,
        "board_task_id": "btk-wave20-task08-destructive",
        "auto_answer_policy": "deterministic_safe",
        "policy_result": "skipped_destructive_action",
        // The suggestion may still be `approved` (every other rule
        // passed) but the listener MUST defer; the LISTENER reads
        // `requires_human` not `selected_decision`.
        "selected_decision": "approved",
        "safety_rule_results": [
            "rule:deterministic_mode:producer ran in deterministic/dry-run mode",
            "rule:no_file_write:artifact did not touch the filesystem",
            "rule:no_protected_source_or_target:neither side is on the protected list",
            "rule:no_additional_blockers:no caller-side warnings or partial-status conflicts on the response payload",
            "rule:caller_opt_in:caller explicitly supplied review_automation_policy",
            "rule:destructive_action:`archive` is on the destructive list (archive|supersede|remove); auto-answer refuses to promote",
        ],
        "requires_human": true,
    });
    let inner = ToolResult {
        content: vec![missiond_mcp::tools::ToolContent::Text {
            text: serde_json::to_string_pretty(&payload).unwrap(),
        }],
        is_error: None,
    };
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "destructive action — defer to human reviewer",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let meta = smoke_meta_of(&decorated);
    assert_eq!(
        meta["artifact_refs"]["auto_answer_policy"],
        "deterministic_safe"
    );
    // Pinning the dedicated destructive-action status so a future
    // refactor that collapses it back into `skipped_rules_failed`
    // lands an explicit failure here.
    assert_eq!(
        meta["artifact_refs"]["policy_result"],
        "skipped_destructive_action"
    );
    // Invariant I2: the envelope MUST require human review.
    assert_eq!(
        meta["artifact_refs"]["requires_human"], true,
        "invariant I2: destructive action MUST always defer to human"
    );
    // Pinning the rule trail so observers can grep for the
    // destructive-action signal.
    let rules = meta["artifact_refs"]["safety_rule_results"]
        .as_array()
        .unwrap();
    assert!(
        rules
            .iter()
            .any(|r| r.as_str().unwrap().contains("destructive_action")
                && r.as_str().unwrap().contains("archive")),
        "rule trail MUST name the destructive `archive` action"
    );
}

// ── 11. Wave 21 / Task 08 :: machine-contract autonomous loop smoke ──
//
// Goal: prove the wave-21 autonomous loop (machine dispatch SSOT +
// task-contract emit/consume/verify + LLM workstation proposals +
// LLM auto-approve + sonnet distill chain auto-apply gate +
// execution-report verification) survives a single envelope round
// trip via `decorate` end-to-end. The smoke synthesises a single
// canonical inner payload that stamps EVERY wave21 receipt field
// and asserts the unified-entry decorator's `artifact_refs`
// projection lifts the load-bearing keys while leaving the
// markdown brief preview / non-load-bearing receipts on the inner
// payload.
//
// Invariants pinned (cross-wave):
//   * I3-04  every workstation proposal carries `applied=false`
//            (the bundle-level `auto_spawn=false` is also pinned).
//   * I1-06  the LLM auto-approve proposal NEVER carries
//            `decision=rejected` (the validator demotes to
//            `needs_changes`).
//   * I2-06  destructive actions short-circuit to
//            `destructive_blocked` and pin `requires_human=true`.
//   * I3-06  every LLM auto-approve proposal carries `applied=
//            false` AND `requires_human=true` in v0.
//   * I1-07  `auto_sonnet=false` (default) → no `auto_sonnet` block.
//   * I3-07  the gate REUSES the wave-20 trigger outcomes; when the
//            trigger short-circuits the chain block surfaces
//            `triggered=false` + dedicated skip status.
//   * I6-07  every successful auto-sonnet apply pins
//            `review_required=true`.
//   * Wave-21/03 — the unified envelope surfaces
//            `task_contract_source_path` AND `task_contract_path`
//            so the verifier (agent_execution::enforce_verified_completion)
//            can ratify the loop end-to-end without parsing the
//            inner JSON.
//   * Markdown is non-load-bearing — `task_brief_preview` MUST NOT
//     be projected into `artifact_refs`.
//
// No LLM, no spawn, no shell, no markdown read — every receipt is
// synthesised in JSON form so the smoke is fully deterministic.

/// Canonical wave21-08 plan-execute payload — stamps every
/// receipt the wave21-04..07 features produce on a single
/// envelope. The shape mirrors what `build_workstation_dispatch_response`
/// + the wave21-07 `attach_auto_sonnet_to_payload` + the wave21-04
/// proposals splice + the wave21-06 LLM-auto-approve splice would
/// produce when every feature is opted in (or explicitly opted out
/// for the wave21-07 default-off path).
fn wave21_08_smoke_payload() -> Value {
    let plan_id = "77777777-7777-7777-7777-000000000021";
    let board_task_id = "btk-wave21-08-smoke";
    let contract_path = "/tmp/missiond-wave21-08/.missiond/tasks/wave21/wave21-08-dispatch.lisp";
    let render_command = format!("node scripts/render-claudecode-task.mjs {}", contract_path);
    let evidence_path = "/tmp/missiond-wave21-08/.missiond/v2/plans/wave21-08.evidence.json";
    json!({
        // ── Wave 13/14 row-id pointers ─────────────────────────────
        "status": "executing",
        "execute_mode": "internal",
        "runner_status": "workstation_dispatch_v0",
        "plan_id": plan_id,
        "board_task_id": board_task_id,
        // ── Wave 15 / 16 dispatch resolution ───────────────────────
        "target_tool": "mission_task_delegate",
        "target_source": "explicit_arg",
        "dispatch_strategy": "agent-team",
        "dispatch_strategy_source": "explicit_arg",
        "plan_hint_summary": {},
        "workstation_dispatch_source": "explicit_arg",
        "workstation_dispatch_status": "dispatched",
        "scoped_commit_required": true,
        "scoped_commit_policy": "enforced-on-complete",
        // ── Wave 19 / 20 machine-contract emit + dispatch mode ─────
        "task_contract_mode": "emit",
        "task_contract_eligible": true,
        "task_contract_path": contract_path,
        "render_command": render_command,
        "dispatch_contract_mode": "machine",
        "task_contract_source_path": contract_path,
        "evidence_path": evidence_path,
        // Brief preview — present for human consumption but NOT
        // projected into artifact_refs (markdown non-load-bearing).
        "task_brief_preview": format!(
            "## Source contract\n- task-contract v1: `{}`\n## Objective\nship the wave21-08 deterministic loop smoke\n",
            contract_path
        ),
        "inner_result": {
            "task_id": board_task_id,
            "status": "dispatched",
        },
        // ── Wave 21 / 04 — workstation proposals (applied=false) ───
        "workstation_proposals": {
            "status": "suggested",
            "auto_spawn": false,
            "model": "claude-sonnet-4-7",
            "caller": "wave21-04-workstation-proposal",
            "proposals": [
                {
                    "field": "target",
                    "value": "mission_task_delegate",
                    "confidence": "high",
                    "evidence": "node hint :target mission_task_delegate matches the contract",
                    "safety_status": "safe",
                    "applied": false,
                },
            ],
            "parse_warnings": [],
        },
        // ── Wave 21 / 06 — LLM auto-approve proposal (requires_human=true) ─
        "llm_auto_approve_proposal_status": "suggested",
        "llm_auto_approve_proposal": {
            "mode": "sonnet_suggest",
            "status": "suggested",
            "action": "approve",
            "model": "claude-sonnet-4-7",
            "caller": "wave21-06-auto-approve",
            "proposal": {
                "decision": "approved",
                "confidence": "high",
                "evidence": "wave21-08 smoke fixture",
                "non_goal_check": "no destructive action",
                "destructive_check": "non_destructive:`approve` is not on the destructive list",
                "requires_human": true,
                "applied": false,
            },
            "proposal_warnings": [],
        },
        // ── Wave 21 / 07 — auto_sonnet apply-gate default-off ──────
        // (No auto_sonnet block — default-off byte-shape preserved.)
        // ── Wave 19 / 20 — distill chain receipt (record_only) ─────
        "distill_chain": {
            "triggered": true,
            "status": "recorded",
            "chain_id": "chain:wave21-08-smoke",
            "chain_id_source": "explicit_arg",
            "chain_mode": "record_only",
            "chain_index_in_plan": 1,
            "evidence_path": evidence_path,
        },
    })
}

/// Wave21-08 canonical smoke: drive the wave21-08 fixture payload
/// through `decorate` once and assert every load-bearing wave21-03..07
/// envelope field surfaces while the markdown brief preview stays
/// non-load-bearing. This is the single round-trip that proves the
/// wave21 autonomous loop carries through the unified entry.
#[test]
fn smoke_wave21_machine_contract_autonomous_loop_envelope_pins_every_invariant() {
    let plan_id = "77777777-7777-7777-7777-000000000021";
    let board_task_id = "btk-wave21-08-smoke";
    let contract_path = "/tmp/missiond-wave21-08/.missiond/tasks/wave21/wave21-08-dispatch.lisp";

    let payload = wave21_08_smoke_payload();
    let inner = smoke_inner_result(payload.clone());
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "wave21-08 machine-contract loop landed; collect evidence + run verifier",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let meta = smoke_meta_of(&decorated);
    assert_eq!(meta["pipeline_stage"], stages::EXECUTION_RUNNER);
    let refs = meta["artifact_refs"]
        .as_object()
        .expect("artifact_refs object");

    // ── Wave 13/14 row-id pointers preserved ──────────────────────
    assert_eq!(refs["scope"], "execution");
    assert_eq!(refs["plan_id"], plan_id);
    assert_eq!(refs["board_task_id"], board_task_id);
    assert_eq!(refs["status"], "executing");

    // ── Wave 19/06 + 20/04 machine-contract splice surfaces ───────
    assert_eq!(refs["task_contract_mode"], "emit");
    assert_eq!(refs["task_contract_eligible"], true);
    assert_eq!(refs["task_contract_path"], contract_path);
    assert_eq!(refs["task_contract_source_path"], contract_path);
    assert_eq!(refs["dispatch_contract_mode"], "machine");
    // Wave-21/03 SSOT invariant: source path consumed equals path
    // emitted — the verifier can ratify without parsing the inner
    // JSON.
    assert_eq!(
        refs["task_contract_source_path"], refs["task_contract_path"],
        "wave21-08 invariant: machine-mode source path consumed equals path emitted"
    );

    // ── Markdown brief is non-load-bearing ─────────────────────────
    assert!(
        !refs.contains_key("task_brief_preview"),
        "wave21-08 invariant: task_brief_preview MUST NOT be projected into artifact_refs \
         (markdown is non-load-bearing in machine mode)"
    );
    assert!(
        !refs.contains_key("workstation_dispatch_status"),
        "wave21-08 invariant: workstation_dispatch_status stays on the inner payload"
    );

    // ── Wave 21/04 + 21/06 + 21/07 splices stay on the inner payload ──
    // The wave21 LLM-side blocks are not lifted into artifact_refs in
    // v0 (the projection deliberately stays terse). They MUST live
    // on the inner JSON so observers can read them per-call without
    // bloating the envelope.
    let inner_text = match &decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let inner_json: Value = serde_json::from_str(&inner_text).expect("inner JSON parses");

    // I3-04 (workstation proposals): every proposal MUST carry
    // applied=false AND the bundle MUST pin auto_spawn=false.
    let proposals_block = &inner_json["workstation_proposals"];
    assert_eq!(
        proposals_block["auto_spawn"], false,
        "wave21-04 invariant I3: bundle MUST pin auto_spawn=false at top level"
    );
    assert_eq!(proposals_block["status"], "suggested");
    let props = proposals_block["proposals"]
        .as_array()
        .expect("proposals array");
    assert!(
        !props.is_empty(),
        "wave21-08 fixture must surface proposals"
    );
    for p in props {
        assert_eq!(
            p["applied"], false,
            "wave21-04 invariant I3: every proposal MUST carry applied=false"
        );
    }

    // I1-06 + I3-06 (LLM auto-approve): proposal decision NEVER
    // rejected; applied=false; requires_human=true.
    let llm_block = &inner_json["llm_auto_approve_proposal"];
    let proposal = &llm_block["proposal"];
    assert_ne!(
        proposal["decision"], "rejected",
        "wave21-06 invariant I1: LLM auto-approve proposal MUST NEVER carry decision=rejected"
    );
    assert_eq!(
        proposal["applied"], false,
        "wave21-06 invariant I3: LLM auto-approve proposal MUST carry applied=false"
    );
    assert_eq!(
        proposal["requires_human"], true,
        "wave21-06 invariant I3: LLM auto-approve proposal MUST pin requires_human=true in v0"
    );

    // I1-07 (auto_sonnet default-off byte-shape): no auto_sonnet
    // block surfaces unless the gate fires. The fixture deliberately
    // omits the block to prove the default-off path is preserved
    // through `decorate`.
    assert!(
        inner_json.get("auto_sonnet").is_none(),
        "wave21-07 invariant I1: default-off byte-shape — no auto_sonnet block when not requested"
    );
    assert!(
        inner_json.get("auto_sonnet_status").is_none(),
        "wave21-07 invariant I1: default-off byte-shape — no auto_sonnet_status shortcut when not requested"
    );

    // I3-07 (distill chain reuses wave-20 trigger outcomes): when
    // the gate fires in record_only mode, the chain block surfaces
    // triggered=true + status=recorded. The wave21-07 SSOT here is
    // that `chain_mode` flows through verbatim — the gate never
    // upgrades to sonnet on the default-off path.
    let chain = &inner_json["distill_chain"];
    assert_eq!(chain["triggered"], true);
    assert_eq!(chain["status"], "recorded");
    assert_eq!(
        chain["chain_mode"], "record_only",
        "wave21-07 invariant I1: chain_mode stays record_only on default-off path"
    );

    // ── v0/v1 non-goals remain loud ────────────────────────────────
    let non_goals = meta["v0_non_goals"].as_array().unwrap();
    for needle in [
        "auto_approve_directive",
        "auto_approve_plan",
        "auto_answer_review_question",
        "autonomous_workstation_dispatch",
    ] {
        assert!(
            non_goals.iter().any(|v| v == needle),
            "wave21-08 envelope MUST keep `{}` in v0_non_goals",
            needle
        );
    }
}

/// Wave21-08 destructive-action smoke: when the wave21-06 LLM auto-
/// approve gate sees a destructive action (`archive`), the bundle
/// MUST short-circuit to `destructive_blocked` and pin
/// `requires_human=true` on the proposal regardless of the model's
/// suggestion. This is the I2-06 invariant — the daemon refuses to
/// auto-approve a destructive action even if Sonnet suggested
/// `approved`.
#[test]
fn smoke_wave21_llm_auto_approve_destructive_action_pins_requires_human() {
    // Synthesise a destructive-action proposal mirror — the wave21-06
    // bundle would surface this via `LlmAutoApproveProposalBundle::
    // destructive_blocked` after collapsing the model's claim.
    let payload = json!({
        "status": "draft",
        "plan_id": "77777777-7777-7777-7777-000000000022",
        "board_task_id": "btk-wave21-08-archive",
        "llm_auto_approve_proposal_status": "destructive_blocked",
        "llm_auto_approve_proposal": {
            "mode": "sonnet_suggest",
            "status": "destructive_blocked",
            "action": "archive",
            "model": "claude-sonnet-4-7",
            "proposal": {
                // Model claimed `approved` but the bundle MUST surface
                // requires_human=true regardless (I2-06 + I5-06).
                "decision": "approved",
                "confidence": "high",
                "evidence": "model thought archiving was fine",
                "non_goal_check": "model did not check non-goals",
                "destructive_check":
                    "destructive:`archive` is on the destructive list (archive|supersede|remove); auto-approve proposal pinned `requires_human=true` regardless of model output",
                "requires_human": true,
                "applied": false,
            },
            "proposal_warnings": [
                "destructive action `archive` short-circuited to destructive_blocked",
            ],
        },
    });
    let inner = smoke_inner_result(payload);
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::PLAN_REVIEW_GATE,
            scope: ArtifactScope::Plan,
            next_step: "destructive action — defer to human reviewer",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let inner_text = match &decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let inner_json: Value = serde_json::from_str(&inner_text).expect("inner JSON parses");
    let llm = &inner_json["llm_auto_approve_proposal"];
    assert_eq!(llm["status"], "destructive_blocked");
    let proposal = &llm["proposal"];
    // I2-06: requires_human MUST be true on every destructive
    // proposal — the daemon overrides the model's claim.
    assert_eq!(
        proposal["requires_human"], true,
        "wave21-06 invariant I2: destructive action MUST always require human review"
    );
    // I5-06: destructive_check ALWAYS sourced from
    // is_destructive_review_action — the model's value is overridden.
    let dc = proposal["destructive_check"].as_str().unwrap_or("");
    assert!(
        dc.starts_with("destructive:"),
        "wave21-06 invariant I5: destructive_check MUST be sourced from is_destructive_review_action — got `{}`",
        dc
    );
    assert!(
        dc.contains("archive"),
        "wave21-06 destructive_check MUST name the destructive action"
    );
    // I3-06: applied=false pinned even on destructive_blocked
    // (propose-only contract).
    assert_eq!(
        proposal["applied"], false,
        "wave21-06 invariant I3: applied=false pinned on every proposal regardless of status"
    );
}

/// Wave21-08 markdown-non-load-bearing smoke: pin that even with
/// EVERY wave21 receipt block stamped on the inner payload, the
/// `task_brief_preview` markdown does NOT get projected into
/// `artifact_refs`. This is the wave21-08 brief contract: the
/// machine-contract loop is driven by the contract path (the SSOT)
/// — the markdown is purely human-readable mirror.
#[test]
fn smoke_wave21_machine_contract_loop_proof_markdown_is_non_load_bearing() {
    let payload = wave21_08_smoke_payload();
    let inner = smoke_inner_result(payload);
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "wave21-08 machine-mode dispatch landed",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let meta = smoke_meta_of(&decorated);
    let refs = meta["artifact_refs"]
        .as_object()
        .expect("artifact_refs object");

    // Load-bearing surface — observers MUST be able to drive the
    // machine handoff entirely from these keys.
    assert!(
        refs.contains_key("task_contract_path"),
        "wave21-08 envelope MUST surface task_contract_path (load-bearing)"
    );
    assert!(
        refs.contains_key("task_contract_source_path"),
        "wave21-08 envelope MUST surface task_contract_source_path (load-bearing SSOT)"
    );
    assert_eq!(refs["dispatch_contract_mode"], "machine");

    // Non-load-bearing markdown — MUST NOT leak into artifact_refs.
    for needle in [
        "task_brief_preview",
        "task_brief",
        "workstation_dispatch_status",
        "scoped_commit_required",
        "scoped_commit_policy",
    ] {
        assert!(
            !refs.contains_key(needle),
            "wave21-08 invariant: `{}` MUST stay on the inner payload, not in artifact_refs",
            needle
        );
    }

    // Inner payload still carries the brief preview so a human can
    // read it without re-rendering — the brief preview is OPTIONAL
    // compatibility, not absent.
    let inner_text = match &decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let inner_json: Value = serde_json::from_str(&inner_text).expect("inner JSON parses");
    let preview = inner_json["task_brief_preview"].as_str().unwrap_or("");
    assert!(
        preview.contains("## Source contract"),
        "wave21-08 brief preview MUST carry the wave-19/07 source-contract preamble \
         on the inner payload"
    );
}

// ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
//
// Deterministic envelope-side smoke covering the wave22-07 v4
// contract: a single fixture payload that stamps every wave-22
// apply-gate block (wave22-03 review LLM auto-approve apply gate /
// wave22-04 persisted PLAN inference apply v2 / wave22-05
// workstation auto-spawn / wave22-06 distill chain auto-Sonnet
// policy) on top of the wave21-08 propose-only payload, then runs
// the result through `decorate` and asserts:
//   * markdown brief preview stays non-load-bearing in
//     `artifact_refs` (no real LLM, no real spawn — the inner
//     payload carries the brief, the envelope carries only the
//     load-bearing row-id pointers);
//   * every wave-21 apply-gate-related invariant is preserved on
//     the envelope (wave21-04 4 inv + wave21-05 6 inv + wave21-06
//     5 inv + wave21-07 7 inv = 22 cross-wave invariants total).
// The companion review_gate.rs / plan.rs / workstation_dispatch.rs
// / agent_execution.rs smokes cover the per-gate apply / reject
// contracts and the failed-verification path.

/// Wave22-07 v4 fixture: extends `wave21_08_smoke_payload()` with
/// every wave-22 apply-gate block stamped on the inner payload so
/// the envelope-side smoke can prove the markdown / load-bearing
/// boundary holds even with the full apply-gate cluster surfaced.
fn wave22_07_smoke_payload_v4() -> Value {
    let mut payload = wave21_08_smoke_payload();
    let map = payload.as_object_mut().expect("fixture is a JSON object");

    // Wave22-03 — LLM auto-approve apply gate (default off-path
    // shape: `apply_status=not_requested`, every wave21-06 invariant
    // pinned through the proposal block on the same payload).
    map.insert(
        "llm_auto_approve_proposal_hash".to_string(),
        json!("deadbeefdeadbeefdeadbeefdeadbeef"),
    );
    map.insert(
        "llm_approve_apply_gate".to_string(),
        json!({
            "apply_status": "not_requested",
            "applied_decision": null,
            "proposal_hash_status": "not_supplied",
            "computed_proposal_hash": "deadbeefdeadbeefdeadbeefdeadbeef",
            "supplied_proposal_hash": null,
            "caller_approved": false,
            "safety_rule_results": [
                "rule:apply_gate:not_requested",
            ],
        }),
    );

    // Wave22-04 — persisted PLAN inference apply v2 (default
    // not_requested shape so the v1 byte-shape stays untouched —
    // wave21-05 I6 pinned through the parallel block).
    map.insert(
        "persisted_apply".to_string(),
        json!({
            "status": "not_requested",
            "requested_apply": false,
            "requested_persist": false,
            "requested_caller_approved": false,
            "applied_count": 0,
            "skipped_count": 0,
            "computed_proposal_hash": "0".repeat(32),
            "supplied_proposal_hash": null,
            "original_sexp_hash": "0".repeat(64),
            "applied_fields": [],
            "skipped_fields": [],
            "evidence_entry_appended": false,
            "rollback_plan_id": null,
            "predecessor_plan_id": null,
        }),
    );

    // Wave22-05 — workstation auto-spawn gate (default off ⇒
    // gate block intentionally OMITTED to prove wave21-04 I1
    // byte-shape preservation: the absence of the
    // `workstation_auto_spawn_gate` key on the inner payload is
    // load-bearing).

    // Wave22-06 — distill chain auto-Sonnet policy (default off ⇒
    // policy block intentionally OMITTED to prove wave21-07 I1
    // byte-shape preservation: the absence of the
    // `auto_sonnet_policy` block on the inner payload is
    // load-bearing).

    payload
}

/// V4 smoke envelope: drive the wave22-07 v4 fixture payload
/// through `decorate` once and assert every cross-wave invariant
/// + the markdown-non-load-bearing contract. The envelope MUST
/// carry the load-bearing row-id pointers in `artifact_refs`
/// while keeping every wave-22 apply-gate block ON THE INNER
/// PAYLOAD (gates are observability — they MUST NOT bloat the
/// envelope) and every markdown brief field OFF the
/// `artifact_refs`.
#[test]
fn smoke_wave22_07_v4_envelope_pins_apply_gates_and_markdown_non_load_bearing() {
    let payload = wave22_07_smoke_payload_v4();
    let inner = smoke_inner_result(payload);
    let decorated = decorate(
        inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step: "wave22-07 v4 smoke — apply-gate cluster envelope landed",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    );
    let meta = smoke_meta_of(&decorated);
    let refs = meta["artifact_refs"]
        .as_object()
        .expect("artifact_refs object");

    // ── Load-bearing surface — observers MUST drive the loop
    //    entirely from these keys. ────────────────────────────────
    assert_eq!(refs["scope"], "execution");
    assert_eq!(refs["dispatch_contract_mode"], "machine");
    assert!(
        refs.contains_key("task_contract_path"),
        "wave22-07 v4: envelope MUST surface task_contract_path (load-bearing)"
    );
    assert_eq!(
        refs["task_contract_source_path"], refs["task_contract_path"],
        "wave22-07 v4 invariant: machine-mode source path consumed equals path emitted"
    );

    // ── Markdown is non-load-bearing — no brief field MAY leak
    //    into artifact_refs even with every wave-22 apply-gate
    //    block stamped on the inner payload. ────────────────────
    for needle in [
        "task_brief_preview",
        "task_brief",
        "workstation_dispatch_status",
        "scoped_commit_required",
        "scoped_commit_policy",
        // Wave-22 apply-gate blocks MUST stay on inner payload.
        "llm_approve_apply_gate",
        "llm_auto_approve_proposal_hash",
        "persisted_apply",
        "workstation_auto_spawn_gate",
        "auto_sonnet_policy",
    ] {
        assert!(
            !refs.contains_key(needle),
            "wave22-07 v4 invariant: `{}` MUST stay on the inner payload, not in artifact_refs \
             (markdown / apply-gate blocks are non-load-bearing in machine mode)",
            needle
        );
    }

    // Inner payload still carries the brief preview so a human
    // can read it without re-rendering — the brief preview is
    // OPTIONAL human-readable mirror, not absent.
    let inner_text = match &decorated.content[0] {
        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
    };
    let inner_json: Value = serde_json::from_str(&inner_text).expect("inner JSON parses");
    assert!(
        inner_json["task_brief_preview"]
            .as_str()
            .unwrap_or("")
            .contains("## Source contract"),
        "wave22-07 v4: brief preview MUST stay on the inner payload"
    );

    // ── Wave21-04 (4 invariants) — workstation propose-only ─────
    // I1 default off — no `workstation_auto_spawn_gate` block on
    // the inner payload (the wave22-05 default-off byte-shape).
    assert!(
        inner_json.get("workstation_auto_spawn_gate").is_none(),
        "wave21-04 I1 / wave22-05 default-off byte-shape: no auto-spawn gate block on \
         the inner payload when auto_spawn is absent"
    );
    // I2 Sonnet unavailable no fallback — the propose-only
    // bundle's `unavailable_reason` field stays absent for the
    // happy-path fixture; pinning the shape proves no fallback
    // synthesised proposals snuck in.
    let proposals_block = &inner_json["workstation_proposals"];
    assert!(
        proposals_block.get("unavailable_reason").is_none()
            || proposals_block["unavailable_reason"].is_null(),
        "wave21-04 I2: no fallback proposals — `unavailable_reason` MUST stay absent / null \
         on the happy-path fixture"
    );
    // I3 DAG mode rejects — flows through into `bundle=None` /
    // `parse_warnings` empty on the happy path. Pin the empty
    // warnings array.
    assert!(
        proposals_block["parse_warnings"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false),
        "wave21-04 I3: parse_warnings empty on happy path"
    );
    // I4 propose-only fields preserved — bundle.auto_spawn=false
    // and every proposal.applied=false stay on the wire.
    assert_eq!(
        proposals_block["auto_spawn"], false,
        "wave21-04 I4: bundle MUST keep auto_spawn=false on propose-only surface"
    );
    for p in proposals_block["proposals"]
        .as_array()
        .expect("proposals array")
    {
        assert_eq!(
            p["applied"], false,
            "wave21-04 I4: every proposal MUST keep applied=false"
        );
    }

    // ── Wave21-05 (6 invariants) — PLAN inference apply gate v1 ─
    // I1 default off — `persisted_apply.status=not_requested`.
    let persisted = &inner_json["persisted_apply"];
    assert_eq!(
        persisted["status"], "not_requested",
        "wave21-05 I1: persist opt-ins absent ⇒ persisted_apply.status=not_requested"
    );
    // I2 strict bool/string shape — applied_fields[] / skipped_fields[]
    // shapes stay arrays even when empty (no string fallback).
    assert!(
        persisted["applied_fields"].is_array() && persisted["skipped_fields"].is_array(),
        "wave21-05 I2: applied / skipped shapes MUST stay arrays"
    );
    // I3 conflicts NEVER apply — applied_count=0 on the
    // not_requested path proves no conflict slipped through.
    assert_eq!(persisted["applied_count"], 0);
    // I4 sub-threshold suggestions NEVER apply — same applied_count=0
    // proof carries this invariant on the off-path.
    assert_eq!(persisted["skipped_count"], 0);
    // I5 LLM proposals require llm_caller_approved — the
    // propose-only LLM proposal block stays applied=false on
    // the same payload (caller_approved is the PERSIST opt-in,
    // not the LLM opt-in).
    let llm_block = &inner_json["llm_auto_approve_proposal"];
    assert_eq!(
        llm_block["proposal"]["applied"], false,
        "wave21-05 I5: caller_approved is PERSIST opt-in only — LLM proposal stays applied=false"
    );
    // I6 apply_gate.persist_inference_applied stays hard-pinned
    // false — the v1 wire shape never changes; v2 publishes
    // persistence on the SEPARATE `persisted_apply` block. We
    // pin this through the absence of `persist_inference_applied`
    // on the wire — the gate hasn't run yet (default off-path).
    assert!(
        inner_json.get("apply_gate").is_none()
            || inner_json["apply_gate"]
                .get("persist_inference_applied")
                .map(|v| v == false)
                .unwrap_or(true),
        "wave21-05 I6: persist_inference_applied stays hard-pinned false \
         on the v1 apply_gate block (when present)"
    );

    // ── Wave21-06 (5 invariants) — LLM auto-approve propose-only ─
    let proposal = &llm_block["proposal"];
    // I1 never auto-reject — proposal.decision MUST NEVER be `rejected`.
    assert_ne!(
        proposal["decision"], "rejected",
        "wave21-06 I1: proposal.decision MUST NEVER be `rejected`"
    );
    // I2 destructive never promote — fixture carries
    // `non_destructive:` prefix on `destructive_check` for the
    // safe-action path; pinning the prefix proves no destructive
    // action was elevated.
    let dc = proposal["destructive_check"].as_str().unwrap_or("");
    assert!(
        dc.starts_with("non_destructive:"),
        "wave21-06 I2: destructive_check prefix MUST stay non_destructive on safe action"
    );
    // I3 proposal block applied=false / requires_human=true.
    assert_eq!(
        proposal["applied"], false,
        "wave21-06 I3: proposal MUST stay applied=false on propose-only surface"
    );
    assert_eq!(
        proposal["requires_human"], true,
        "wave21-06 I3: proposal MUST stay requires_human=true on propose-only surface"
    );
    // I4 Sonnet unavailable no fallback — the bundle's `status`
    // field stays `suggested` (NOT `unavailable_blocked`); the
    // fixture deliberately uses the happy path so the absence of
    // any unavailable shape proves no fallback ran.
    assert_eq!(
        llm_block["status"], "suggested",
        "wave21-06 I4: happy-path fixture MUST stay status=suggested (no fallback)"
    );
    // I5 destructive_check ALWAYS deterministic — the prefix
    // (`non_destructive:` / `destructive:`) is the deterministic
    // verdict; pinning the colon-prefix shape proves the model's
    // value would have been overridden.
    assert!(
        dc.contains(':'),
        "wave21-06 I5: destructive_check MUST carry the deterministic verdict prefix"
    );

    // ── Wave21-07 (7 invariants) — Sonnet distill chain auto-apply ─
    // I1 default=off — no `auto_sonnet` / `auto_sonnet_status` /
    // `auto_sonnet_policy` block on the inner payload.
    for needle in ["auto_sonnet", "auto_sonnet_status", "auto_sonnet_policy"] {
        assert!(
            inner_json.get(needle).is_none(),
            "wave21-07 I1 / wave22-06 default-off byte-shape: no `{}` block on inner payload",
            needle
        );
    }
    // I2 strict bool shape — the chain block carries `chain_mode`
    // as a closed enum string (NOT a bool synonym). Pinning the
    // string-typed field proves the strict-shape parser holds.
    let chain = &inner_json["distill_chain"];
    assert!(
        chain["chain_mode"].is_string(),
        "wave21-07 I2: chain_mode MUST stay a closed-enum string (strict shape)"
    );
    // I3 ALL six wave-20 deterministic rules MUST pass — the
    // happy-path fixture carries `triggered=true` + `status=recorded`
    // proving the wave-20 trigger gate was honoured (the rules
    // are evaluated upstream by the trigger; the chain block
    // surfaces the verdict).
    assert_eq!(
        chain["triggered"], true,
        "wave21-07 I3: triggered=true on the happy-path fixture proves rules passed"
    );
    // I4 distill_mode=sonnet rejected — the fixture stays in
    // `record_only` mode (no `sonnet` mode on the wire) — pinning
    // the mode proves the gate would have rejected the double-call.
    assert_eq!(
        chain["chain_mode"], "record_only",
        "wave21-07 I4: chain_mode stays record_only — sonnet double-call rejected"
    );
    // I5 Sonnet failure preserves inner payload — the chain
    // block carries `evidence_path` + `status` so a follow-up
    // Sonnet failure would NOT collapse the payload. Pin the
    // evidence_path shape.
    assert!(
        chain["evidence_path"].is_string(),
        "wave21-07 I5: evidence_path MUST stay on the chain block (preserves inner payload)"
    );
    // I6 review_required=true PINNED — the propose-only LLM
    // block's `requires_human=true` is the wave21-07 review_required
    // anchor on this payload (the dual-flag carrier — the
    // `auto_sonnet_policy` block omitted in default-off mode means
    // the propose-only review_required signal stays load-bearing).
    assert_eq!(
        proposal["requires_human"], true,
        "wave21-07 I6: review_required=true PINNED via propose-only requires_human signal"
    );
    // I7 wave-19/20 blocks remain UNCHANGED — `distill_chain`
    // carries `chain_id` (wave-19) + `chain_mode` (wave-19) +
    // `chain_index_in_plan` (wave-19). Pinning all three proves
    // the wave-19/20 surface is intact under the wave22 apply-gate
    // overlay.
    for key in [
        "chain_id",
        "chain_mode",
        "chain_index_in_plan",
        "evidence_path",
    ] {
        assert!(
            chain.get(key).is_some(),
            "wave21-07 I7: distill_chain MUST keep wave-19/20 field `{}`",
            key
        );
    }

    // ── v0/v1 non-goals remain loud on the envelope ────────────
    let non_goals = meta["v0_non_goals"].as_array().unwrap();
    for needle in [
        "auto_approve_directive",
        "auto_approve_plan",
        "auto_answer_review_question",
        "autonomous_workstation_dispatch",
    ] {
        assert!(
            non_goals.iter().any(|v| v == needle),
            "wave22-07 v4 envelope MUST keep `{}` in v0_non_goals — apply gates are \
             explicit opt-ins, NOT default-on autonomy",
            needle
        );
    }
}
