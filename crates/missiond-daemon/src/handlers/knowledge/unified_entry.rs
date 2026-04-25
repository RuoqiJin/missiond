//! unified_entry — internal pipeline planner for the canonical
//! `message → directive → review → plan → review → execute` flow.
//!
//! Lisp authority:
//!   - intent-flow.lisp ::
//!       F-intent-alignment-plan-execution-loop ::
//!         s1 message-intake + s2 intent-alignment-authoring +
//!         s3 alignment-review-gate + s4 plan-authoring +
//!         s5 plan-review-gate + s6 execution-runner
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline
//!   - intent-tools.lisp :: implemented-surface mission_directive / mission_plan
//!     (this helper deliberately does NOT introduce a new MCP surface — it
//!     sequences the existing manager surfaces)
//!
//! Scope (wave-13 / Task 03 :: unified entry flow v0):
//!
//!   The wave-11/12 work shipped three already-callable manager surfaces:
//!     * `mission_directive(action=compile, …)`     — directive-compiler v0
//!     * `mission_plan(action=compile, …)`          — plan-compiler v0
//!     * `mission_plan(action=execute, …)`          — plan-runner v0+v1+v2
//!
//!   v0 of the unified entry is therefore **strictly composition-only**:
//!     1. caller hands us a message + (optional) approved-id breadcrumbs;
//!     2. we *plan* the next step the caller must take;
//!     3. when the caller already supplies the approved plan id + the
//!        `execute=true` flag, we drive the existing `mission_plan` execute
//!        branch from inside the daemon (no autonomous workstation dispatch);
//!     4. every response carries `pipeline_stage` + `next_step` + (when
//!        meaningful) `next_call` so the caller never has to guess the
//!        canonical sequence.
//!
//!   Things v0 deliberately does NOT do — these are explicit non-goals:
//!     * auto-approve a directive       (s3 is a human/Codex review gate)
//!     * auto-approve a plan            (s5 is a human/Codex review gate)
//!     * auto-answer a review question  (review-gate emission stays opt-in
//!                                       on the underlying handlers)
//!     * autonomous workstation dispatch (no fresh slot spawn, no ClaudeCode
//!                                        worker enqueue — only mission_plan
//!                                        execute on an already-approved plan)
//!
//!   Architecture decision: NO new MCP tool. The planner is a pure internal
//!   helper. Adding a `mission_message` / `mission_invoke` would duplicate
//!   what `mission_directive` + `mission_plan` already cover; the only
//!   genuinely new behaviour is the *sequencing*, which doesn't need its
//!   own DB row, schema, or tool list slot. If a future iteration needs a
//!   single-call "everything-at-once" surface, it can be layered on top of
//!   this helper without changing the contract here.
//!
//! wave-13 / Task 03 v0 :: this is an *internal* helper. The pure planner
//! surfaces (PipelineDecision / PlannerError / plan_pipeline / build_*)
//! are exhaustively unit-tested below, but the async stage runners
//! (run_pipeline / run_*_stage / decorate / planner_error_response) only
//! fire when a future caller wires this helper into a dispatch path.
//! Until that wiring lands, the dead-code lint would report every
//! pub(crate) symbol; we silence it at the module boundary so the helper
//! can ship + be exercised by tests today without touching `dispatch_tool`.
//! Re-evaluate this attribute the moment a caller is added.
#![allow(dead_code)]

use anyhow::Result;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::state::AppState;

// ───────────────────────────────────────────────────────────────────────
// Stage labels — keep these as constants so tests can pin them and so the
// flow narrative in intent-flow.lisp stays trivially greppable.
// ───────────────────────────────────────────────────────────────────────

/// `pipeline_stage` values returned in the response payload. These line up
/// 1:1 with `F-intent-alignment-plan-execution-loop` stage ids in
/// `intent-flow.lisp`. We expose them as constants because both the
/// planner and its tests pin these strings.
pub(crate) mod stages {
    pub(crate) const MESSAGE_INTAKE: &str = "s1_message_intake";
    pub(crate) const DIRECTIVE_REVIEW_GATE: &str = "s3_alignment_review_gate";
    pub(crate) const PLAN_AUTHORING: &str = "s4_plan_authoring";
    pub(crate) const PLAN_REVIEW_GATE: &str = "s5_plan_review_gate";
    pub(crate) const EXECUTION_RUNNER: &str = "s6_execution_runner";
}

/// `flow_ref` echoed on every response so callers can correlate a unified
/// entry response back to the canonical flow narrative.
const FLOW_REF: &str = "F-intent-alignment-plan-execution-loop";

// ───────────────────────────────────────────────────────────────────────
// Public entry — composes the existing handlers without owning a new MCP
// surface. The caller is responsible for invoking this from wherever
// they want the pipeline narrative; we currently expose it only inside
// the daemon (no `dispatch_tool` route) — this is intentional, see
// module doc.
// ───────────────────────────────────────────────────────────────────────

/// Run one tick of the unified entry pipeline.
///
/// Inputs (all read from `args`, all but `message` optional):
///   * `message`              — raw user utterance / external request body
///   * `source`               — provenance string forwarded to the directive
///                              compiler (defaults to "user_utterance")
///   * `conversation_id`      — opt-in correlation id for the directive row
///   * `compiler_mode`        — "dry_run" (default) | "sonnet" — forwarded
///                              to both directive and plan compilers
///   * `persist`              — whether the directive / plan compile call
///                              should write a draft row (defaults to false)
///   * `directive_*`          — caller-side breadcrumbs for the directive
///                              compile call (review_gate / affected_pillars
///                              / non_goals / acceptance) — opaque pass-through
///   * `approved_directive_id` + `directive_version`
///                            — when present, the planner moves to s4 and
///                              calls `mission_plan(action=compile)`. The
///                              underlying plan compiler enforces the
///                              "directive must be approved/compiled" gate
///                              (no allow_unapproved override here).
///   * `board_task_id`        — required when advancing to s4 (PLAN row anchor)
///   * `plan_*`               — caller-side breadcrumbs for the plan compile
///                              call (acceptance / constraints / target_project /
///                              dispatch_strategy / parallelism)
///   * `approved_plan_id`     — when present + `execute=true`, the planner
///                              moves to s6 and calls `mission_plan(action=execute)`
///                              with whatever execute-knobs the caller supplied
///                              (`execute_mode`, `scheduler_mode`,
///                              `max_parallel_nodes`, `target`, …)
///   * `execute`              — gating flag for the s6 transition. Defaults
///                              to `false` so a stray `approved_plan_id`
///                              never silently triggers execution.
///   * `execute_after_approval`
///                            — alias of `execute` (we accept either name
///                              for ergonomic reasons; the task spec uses
///                              the longer form).
pub(crate) async fn run_pipeline(state: &AppState, args: Value) -> Result<ToolResult> {
    let plan_pre = match plan_pipeline(&args) {
        Ok(p) => p,
        Err(e) => return Ok(planner_error_response(e)),
    };

    match plan_pre {
        PipelineDecision::DirectiveCompile { compile_args } => {
            run_directive_compile_stage(state, compile_args).await
        }
        PipelineDecision::PlanCompile { compile_args } => {
            run_plan_compile_stage(state, compile_args).await
        }
        PipelineDecision::PlanExecute { execute_args } => {
            run_plan_execute_stage(state, execute_args).await
        }
    }
}

// ───────────────────────────────────────────────────────────────────────
// Pure planner — picks the next stage given caller inputs. Pulled out of
// the async path so unit tests can pin the routing logic without touching
// AppState / DB / Sonnet.
// ───────────────────────────────────────────────────────────────────────

/// What the planner decided to do next, expressed as the JSON payload to
/// forward to the underlying handler. Each variant maps to exactly one
/// existing manager-surface action:
///   * `DirectiveCompile` → `directive::handle(action=compile, …)`
///   * `PlanCompile`      → `plan::handle(action=compile, …)`
///   * `PlanExecute`      → `plan::handle(action=execute, …)`
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PipelineDecision {
    DirectiveCompile { compile_args: Value },
    PlanCompile { compile_args: Value },
    PlanExecute { execute_args: Value },
}

/// Errors the planner surfaces *before* hitting any DB / LLM call. These
/// become structured `ToolError`s in the response. Pulling them out as a
/// typed enum keeps `plan_pipeline` pure — the test suite asserts on the
/// variants directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlannerError {
    /// Caller hit s1 without supplying any of the upstream signals the
    /// planner needs. We never silently default `message=""` because that
    /// would push noise straight into the LLM.
    MissingMessage,
    /// Caller declared an `approved_directive_id` but skipped the
    /// `board_task_id` that the plan-compiler anchors against — fail
    /// loudly per CLAUDE.md `feedback_fail_fast_no_fallback`, do not
    /// invent a placeholder.
    PlanCompileMissingBoardTask,
    /// Caller declared `execute=true` without an `approved_plan_id`. This
    /// is the safety check that prevents s6 from firing on a still-being-
    /// reviewed plan.
    ExecuteWithoutApprovedPlan,
    /// `approved_plan_id` is present but the caller forgot the `execute`
    /// flag. We refuse to autodetect-and-execute — the explicit flag is
    /// the human-in-the-loop checkpoint.
    ApprovedPlanWithoutExecuteFlag,
}

impl PlannerError {
    fn code(&self) -> &'static str {
        match self {
            Self::MissingMessage => error_codes::MISSING_PARAM,
            Self::PlanCompileMissingBoardTask => error_codes::MISSING_PARAM,
            Self::ExecuteWithoutApprovedPlan => error_codes::MISSING_PARAM,
            Self::ApprovedPlanWithoutExecuteFlag => error_codes::INVALID_PARAM,
        }
    }

    fn message(&self) -> &'static str {
        match self {
            Self::MissingMessage =>
                "unified entry pipeline requires a non-empty `message` to seed s1 message-intake",
            Self::PlanCompileMissingBoardTask =>
                "advancing past s3 to s4 plan-authoring requires `board_task_id` (PLAN.lisp anchors against it; planner refuses to fabricate one)",
            Self::ExecuteWithoutApprovedPlan =>
                "execute=true requested but no `approved_plan_id` provided; s6 execution-runner refuses to dispatch without an explicit approved plan id",
            Self::ApprovedPlanWithoutExecuteFlag =>
                "`approved_plan_id` provided without `execute=true`; v0 unified entry does NOT auto-execute — set execute=true (or execute_after_approval=true) to dispatch s6",
        }
    }

    fn suggestion(&self) -> &'static str {
        match self {
            Self::MissingMessage =>
                "pass `message` (the user utterance / external request body) at minimum",
            Self::PlanCompileMissingBoardTask =>
                "create a board task first via mission_board_create, then re-call with board_task_id",
            Self::ExecuteWithoutApprovedPlan =>
                "complete s4+s5 first (mission_plan compile + approve), then re-call with approved_plan_id",
            Self::ApprovedPlanWithoutExecuteFlag =>
                "re-call with execute=true to dispatch s6, or omit approved_plan_id to stop after s5 review pointer",
        }
    }
}

/// Decide which stage to run based purely on the caller's args.
///
/// Routing precedence (highest first):
///   1. `approved_plan_id` + `execute=true` → s6 execution-runner
///   2. `approved_plan_id` alone            → ERROR (no auto-execute)
///   3. `approved_directive_id`             → s4 plan-authoring
///   4. `message` present                   → s1 message-intake
///   5. nothing usable                      → ERROR (missing message)
pub(crate) fn plan_pipeline(args: &Value) -> std::result::Result<PipelineDecision, PlannerError> {
    let approved_plan_id = nonblank(args.get("approved_plan_id"));
    let approved_directive_id = nonblank(args.get("approved_directive_id"));
    let execute_flag = args
        .get("execute")
        .and_then(|v| v.as_bool())
        .or_else(|| args.get("execute_after_approval").and_then(|v| v.as_bool()))
        .unwrap_or(false);

    // s6 — caller has already cleared s5 and is asking us to execute.
    if approved_plan_id.is_some() {
        if !execute_flag {
            return Err(PlannerError::ApprovedPlanWithoutExecuteFlag);
        }
        return Ok(PipelineDecision::PlanExecute {
            execute_args: build_plan_execute_args(approved_plan_id.unwrap(), args),
        });
    }
    if execute_flag {
        // execute=true but no approved_plan_id — refuse, never silently
        // skip ahead.
        return Err(PlannerError::ExecuteWithoutApprovedPlan);
    }

    // s4 — caller has cleared s3 and wants the planner to compile a plan.
    if let Some(did) = approved_directive_id {
        let board_task_id = nonblank(args.get("board_task_id"));
        if board_task_id.is_none() {
            return Err(PlannerError::PlanCompileMissingBoardTask);
        }
        return Ok(PipelineDecision::PlanCompile {
            compile_args: build_plan_compile_args(did, board_task_id.unwrap(), args),
        });
    }

    // s1 — fresh message coming in, kick off directive compile.
    let message = nonblank(args.get("message"));
    let message = match message {
        Some(m) => m,
        None => return Err(PlannerError::MissingMessage),
    };
    Ok(PipelineDecision::DirectiveCompile {
        compile_args: build_directive_compile_args(message, args),
    })
}

// ───────────────────────────────────────────────────────────────────────
// Argument builders — translate the unified entry args into the args each
// downstream handler already expects. Pure JSON shaping; no IO.
// ───────────────────────────────────────────────────────────────────────

fn build_directive_compile_args(message: String, args: &Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("action".into(), json!("compile"));
    out.insert("utterance".into(), json!(message));

    if let Some(s) = nonblank(args.get("source")) {
        out.insert("source".into(), json!(s));
    }
    if let Some(c) = nonblank(args.get("conversation_id")) {
        out.insert("conversation_id".into(), json!(c));
    }
    if let Some(m) = nonblank(args.get("compiler_mode")) {
        out.insert("compiler_mode".into(), json!(m));
    }
    if let Some(b) = args.get("persist").and_then(|v| v.as_bool()) {
        out.insert("persist".into(), json!(b));
    }
    if let Some(rg) = nonblank(args.get("directive_review_gate")) {
        out.insert("review_gate".into(), json!(rg));
    }
    forward_array(args, "directive_affected_pillars", &mut out, "affected_pillars");
    forward_array(args, "directive_non_goals", &mut out, "non_goals");
    forward_array(args, "directive_acceptance", &mut out, "acceptance");
    Value::Object(out)
}

fn build_plan_compile_args(approved_directive_id: String, board_task_id: String, args: &Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("action".into(), json!("compile"));
    out.insert("directive_id".into(), json!(approved_directive_id));
    out.insert("board_task_id".into(), json!(board_task_id));

    if let Some(v) = args.get("directive_version").and_then(|v| v.as_i64()) {
        out.insert("directive_version".into(), json!(v));
    }
    if let Some(m) = nonblank(args.get("compiler_mode")) {
        out.insert("compiler_mode".into(), json!(m));
    }
    if let Some(b) = args.get("persist").and_then(|v| v.as_bool()) {
        out.insert("persist".into(), json!(b));
    }
    if let Some(tp) = nonblank(args.get("target_project")) {
        out.insert("target_project".into(), json!(tp));
    }
    if let Some(ds) = nonblank(args.get("dispatch_strategy")) {
        out.insert("dispatch_strategy".into(), json!(ds));
    }
    if let Some(p) = nonblank(args.get("parallelism")) {
        out.insert("parallelism".into(), json!(p));
    }
    forward_array(args, "plan_acceptance", &mut out, "acceptance");
    forward_array(args, "plan_constraints", &mut out, "constraints");
    Value::Object(out)
}

fn build_plan_execute_args(approved_plan_id: String, args: &Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("action".into(), json!("execute"));
    out.insert("plan_id".into(), json!(approved_plan_id));

    // Forward execute-time knobs — the underlying mission_plan execute
    // branch already validates these; we don't re-validate here (single
    // source of truth for the execute schema lives in plan.rs).
    for key in [
        "execute_mode",
        "scheduler_mode",
        "max_parallel_nodes",
        "target",
        "dispatch_strategy",
        "target_project",
        "objective",
    ] {
        if let Some(v) = args.get(key) {
            if !v.is_null() {
                out.insert(key.into(), v.clone());
            }
        }
    }
    Value::Object(out)
}

fn forward_array(
    args: &Value,
    src_key: &str,
    out: &mut serde_json::Map<String, Value>,
    dst_key: &str,
) {
    if let Some(arr) = args.get(src_key) {
        if !arr.is_null() {
            out.insert(dst_key.into(), arr.clone());
        }
    }
}

fn nonblank(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ───────────────────────────────────────────────────────────────────────
// Stage runners — thin wrappers that call the existing handlers and then
// decorate the returned `ToolResult` payload with `pipeline_stage` +
// `next_step` + (when meaningful) `next_call`.
//
// We deliberately do NOT mutate any field the underlying handler already
// owns — we only *append* the unified-entry breadcrumbs.
// ───────────────────────────────────────────────────────────────────────

async fn run_directive_compile_stage(state: &AppState, compile_args: Value) -> Result<ToolResult> {
    let inner = super::directive::handle(state, "mission_directive", compile_args).await?;
    Ok(decorate(
        inner,
        stages::MESSAGE_INTAKE,
        "review the compiled directive then re-call this pipeline with `approved_directive_id` (and the directive's `version`) once the human gate passes",
        Some(json!({
            "tool": "mission_directive",
            "action": "approve",
            "note": "approve via mission_directive(action=approve, directive_id=…, version=…). v0 unified entry does NOT auto-approve.",
        })),
        json!({
            "approved_directive_id": "uuid",
            "directive_version": "i32",
            "board_task_id": "string",
        }),
    ))
}

async fn run_plan_compile_stage(state: &AppState, compile_args: Value) -> Result<ToolResult> {
    let inner = super::plan::handle(state, "mission_plan", compile_args).await?;
    Ok(decorate(
        inner,
        stages::PLAN_AUTHORING,
        "review the compiled PLAN.lisp then re-call this pipeline with `approved_plan_id` and `execute=true` once the human gate passes",
        Some(json!({
            "tool": "mission_plan",
            "action": "approve",
            "note": "approve via mission_plan(action=approve, plan_id=…). v0 unified entry does NOT auto-approve.",
        })),
        json!({
            "approved_plan_id": "uuid",
            "execute": true,
        }),
    ))
}

async fn run_plan_execute_stage(state: &AppState, execute_args: Value) -> Result<ToolResult> {
    let inner = super::plan::handle(state, "mission_plan", execute_args).await?;
    Ok(decorate(
        inner,
        stages::EXECUTION_RUNNER,
        "execution dispatched; collect evidence via mission_plan(action=record_evidence) and (when running with auto_distill) mission_workflow(action=distill)",
        None,
        json!({}),
    ))
}

/// Append unified-entry breadcrumbs to whatever the inner handler returned.
///
/// Important: when the inner handler itself returned a `structured_error`,
/// we keep `is_error=true` and *still* surface the stage labels so the
/// caller's pipeline-step UI doesn't lose context. The error payload from
/// the inner handler is nested under `pipeline_inner_error` so callers
/// can introspect both layers.
fn decorate(
    mut inner: ToolResult,
    stage: &str,
    next_step: &str,
    next_call: Option<Value>,
    expects_next_inputs: Value,
) -> ToolResult {
    // We don't reach into the inner ToolResult's structured fields — the
    // public ToolResult contract here is "JSON in the first content
    // element". We append a sibling content element carrying the
    // pipeline metadata; this guarantees zero behavioural change for
    // callers that just want the inner JSON.
    let mut meta = serde_json::Map::new();
    meta.insert("pipeline_stage".into(), json!(stage));
    meta.insert("flow_ref".into(), json!(FLOW_REF));
    meta.insert("next_step".into(), json!(next_step));
    meta.insert("expects_next_inputs".into(), expects_next_inputs);
    if let Some(nc) = next_call {
        meta.insert("next_call".into(), nc);
    }
    meta.insert(
        "v0_non_goals".into(),
        json!([
            "auto_approve_directive",
            "auto_approve_plan",
            "auto_answer_review_question",
            "autonomous_workstation_dispatch",
        ]),
    );

    // ToolResult here is `Vec<ToolContent>`; each entry is opaque to the
    // pipeline planner, so we attach the metadata as a final JSON content
    // element. Keeping inner content untouched preserves any existing
    // structured_error payload.
    inner.content.push(missiond_mcp::tools::ToolContent::Text {
        text: serde_json::to_string_pretty(&Value::Object(meta))
            .unwrap_or_else(|_| "{}".to_string()),
    });
    inner
}

fn planner_error_response(err: PlannerError) -> ToolResult {
    // ToolError schema is `{error_code, reason, suggestion?, trace_id?}` —
    // no `context` slot today. We surface the pipeline-stage breadcrumb
    // by appending a sibling `ToolContent::Text` carrying the metadata,
    // mirroring how `decorate` augments successful responses.
    let tool_err = ToolError::new(err.code(), err.message().to_string())
        .with_suggestion(err.suggestion());
    let mut res = ToolResult::structured_error(tool_err);
    let meta = json!({
        "pipeline_stage": planner_error_stage(&err),
        "flow_ref": FLOW_REF,
        "v0_non_goals": [
            "auto_approve_directive",
            "auto_approve_plan",
            "auto_answer_review_question",
            "autonomous_workstation_dispatch",
        ],
    });
    res.content.push(missiond_mcp::tools::ToolContent::Text {
        text: serde_json::to_string_pretty(&meta).unwrap_or_else(|_| "{}".to_string()),
    });
    res
}

fn planner_error_stage(err: &PlannerError) -> &'static str {
    match err {
        PlannerError::MissingMessage => stages::MESSAGE_INTAKE,
        PlannerError::PlanCompileMissingBoardTask => stages::PLAN_AUTHORING,
        PlannerError::ApprovedPlanWithoutExecuteFlag => stages::PLAN_REVIEW_GATE,
        PlannerError::ExecuteWithoutApprovedPlan => stages::EXECUTION_RUNNER,
    }
}

// ───────────────────────────────────────────────────────────────────────
// tests — pure planner only (no AppState / DB / Sonnet).
//
// The async stage runners are not unit-tested here; they delegate to
// `directive::handle` / `plan::handle`, both of which already have
// extensive integration tests in their own modules. The unique value
// of this helper is the *routing* logic, which is fully covered below.
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
        assert!(err_json["reason"].as_str().unwrap().contains("approved_plan_id"));
        assert!(err_json["suggestion"].as_str().unwrap().contains("execute=true"));

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
        assert_eq!(nonblank(Some(&json!("  hello  "))), Some("hello".to_string()));
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
        assert_eq!(
            out["directive_id"],
            "00000000-0000-0000-0000-000000000abc"
        );
        assert_eq!(out["board_task_id"], "btk-1");
        assert!(out.get("compiler_mode").is_none());
        assert!(out.get("persist").is_none());
        assert!(out.get("acceptance").is_none());
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
        let out = build_plan_execute_args(
            "11111111-1111-1111-1111-111111111111".to_string(),
            &args,
        );
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
        let out = build_plan_execute_args(
            "11111111-1111-1111-1111-111111111111".to_string(),
            &args,
        );
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
}
