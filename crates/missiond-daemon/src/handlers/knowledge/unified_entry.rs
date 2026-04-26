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
//!   - intent-memory.lisp :: directive-layer :: file-first-artifacts
//!     (wave-14 v1 forwards write_file/topic/overwrite_file + project signals)
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
//! Scope (wave-14 / Task 04 :: unified entry pipeline v1):
//!
//!   v1 layers two pieces of forwarding on top of the v0 composition without
//!   adding a new MCP tool. The wave-14/01 file-first writer
//!   (`crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs`)
//!   and wave-14/03 review-gate auto-create (`review_gate.rs`) ship as
//!   opt-in args on the existing directive/plan compile surfaces — the
//!   pipeline simply forwards them so a single unified-entry call can
//!   reach the file-first SSOT mirror + the deterministic review question
//!   id without the caller hand-rolling each downstream call.
//!
//!   v1 forwarding (per stage):
//!     * directive compile  → `write_file / topic / overwrite_file /
//!                              project / cwd / target_project`
//!                            (file-first writer)
//!                          + `review_gate_policy / emit_review_question /
//!                              review_question_text / review_question_id`
//!                            (review-gate auto-create)
//!                          + the wave-13 `directive_*` breadcrumbs
//!     * plan compile       → same file-first + review-gate set; topic
//!                            still defaults to `board_task_id` inside the
//!                            handler when omitted
//!     * plan execute       → same wave-13 execute knobs PLUS `cwd /
//!                              requested_cwd / project / flow_id / params /
//!                              priority / timeout_secs / intent /
//!                              execution_id / parent_design / scope /
//!                              owner / dry_run / review_question_id`
//!                          + wave-17 / task 01 paused-node resume keys
//!                              (`resume_review_question_id /
//!                               resume_review_decision / resume_actor /
//!                               resume_note`); when present the inner
//!                              `mission_plan(action=execute)` routes
//!                              through the deterministic resume helper
//!                              instead of the standard execute pipeline.
//!                          + wave-17 / task 05 finalize / distill opt-ins
//!                              (`finalize_plan / distill_on_success /
//!                               distill_mode`); off by default — when
//!                              `finalize_plan=true` the inner handler
//!                              maps the run aggregate to a final plan
//!                              status and (optionally) triggers the
//!                              workflow distill pass.
//!                          + wave-18 / task 05 cross-plan distill chain
//!                              opt-ins (`distill_chain_id /
//!                               distill_chain_mode / distill_chain_name`);
//!                              forwarded so the inner
//!                              `validate_distill_chain_args` can enforce
//!                              the cross-field "chain knobs require
//!                              finalize_plan=true" rule.
//!                          + wave-18 / task 06 autonomous PLAN field
//!                              inference opt-in (`infer_plan_fields`,
//!                              one of `off|preview|apply_safe`). Default
//!                              `off` on the inner handler; the strict
//!                              allowlist (`parse_infer_plan_fields_mode`)
//!                              fails the call loudly on a typo rather
//!                              than silently degrading to `off`.
//!
//!   Every v1 response now also carries `artifact_refs` — a flat object
//!   that lifts whatever the inner handler produced (`directive_id /
//!   plan_id / version / file_*` / `review_question_*`) so the caller can
//!   correlate the file-first + review-gate state without parsing the inner
//!   payload.
//!
//!   Things v0/v1 deliberately do NOT do — these are explicit non-goals:
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
///
/// wave-14 / Task 04 :: v1 file-first + review-gate forwarding
///   * `write_file` (bool)         — opt into the file-first SSOT mirror on
///                                    directive / plan compile (default false).
///   * `topic` (string)            — file path topic segment; required by the
///                                    directive writer when `write_file=true`,
///                                    optional for the plan writer (defaults
///                                    to `board_task_id` inside the handler).
///   * `overwrite_file` (bool)     — allow replacing an existing artifact
///                                    (default false → atomic refusal).
///   * `project` / `cwd` /
///     `target_project`            — project-root resolution signals (the
///                                    file-first writer rejects process-cwd
///                                    fallback per intent-worker.lisp ::
///                                    project-root-spawn-cwd). Forwarded to
///                                    BOTH the file-first writer (compile)
///                                    AND the execute branch (where the
///                                    plan-runner consumes them — `cwd` is
///                                    also the mission_task_delegate cwd).
///   * `review_gate_policy`        — wave-14/03 policy enum
///                                    (`manual` (default) | `emit_question` |
///                                    `off`). Auto-fires QuestionEvent::
///                                    Created after a successful artifact
///                                    write when set to `emit_question`.
///   * `emit_review_question` /
///     `review_question_text` /
///     `review_question_id`        — wave-11 explicit-emit knobs forwarded
///                                    so callers can also pin the
///                                    deterministic question id.
///
/// Every response now carries `artifact_refs` — a flat object lifting the
/// inner handler's file_* / review_question_* / id+version pointers so the
/// caller can correlate without parsing the inner JSON payload. Legacy
/// callers that omit `write_file` and `review_gate_policy` see the same
/// inert payload as v0 (artifact_refs is still surfaced, but only carries
/// the row-id pointers — no file_* / review_question_* fields are
/// fabricated).
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

    // wave-14 / Task 04 :: file-first SSOT writer pass-through. The directive
    // compiler enforces topic-required when write_file=true (no fallback);
    // we never inject a default here so the failure is loud at the inner
    // handler instead of being silently masked by the pipeline.
    forward_file_first_args(args, &mut out);
    forward_review_gate_args(args, &mut out);
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

    // wave-14 / Task 04 :: file-first SSOT writer pass-through. The plan
    // compiler defaults the topic to `board_task_id` when omitted so the
    // pipeline does not need to inject one — forwarding is straight-through.
    forward_file_first_args(args, &mut out);
    forward_review_gate_args(args, &mut out);
    Value::Object(out)
}

fn build_plan_execute_args(approved_plan_id: String, args: &Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("action".into(), json!("execute"));
    out.insert("plan_id".into(), json!(approved_plan_id));

    // Forward execute-time knobs — the underlying mission_plan execute
    // branch already validates these; we don't re-validate here (single
    // source of truth for the execute schema lives in plan.rs).
    //
    // wave-14 / Task 04 :: extended forwarding so the v1 caller can drive
    // the plan-runner through the unified entry without dropping back to a
    // direct `mission_plan(action=execute)` call. Every key listed here is
    // documented on `mission_plan` (see crates/missiond-mcp/src/tools/
    // knowledge/plan.rs); the pipeline never invents new schema slots.
    for key in [
        // wave-13 v0 keys (preserved)
        "execute_mode",
        "scheduler_mode",
        "max_parallel_nodes",
        "target",
        "dispatch_strategy",
        "target_project",
        "objective",
        // wave-14 v1 additions — runner / dispatcher knobs
        "cwd",
        "requested_cwd",
        "project",
        "execution_id",
        "parent_design",
        "scope",
        "owner",
        "intent",
        "flow_id",
        "params",
        "priority",
        "timeout_secs",
        "dry_run",
        // wave-14 v1 addition — review-gate resolution emit on s6
        "review_question_id",
        // wave-17 / task 01 — paused-node resume hook. The four resume_*
        // keys travel together; the inner `mission_plan(action=execute)`
        // routes through `plan_dag::action_execute_resume` only when
        // `resume_review_question_id` is present. We forward verbatim so
        // unified-entry callers can drive a resume without dropping back
        // to a direct `mission_plan` call.
        "resume_review_question_id",
        "resume_review_decision",
        "resume_actor",
        "resume_note",
        // wave-17 / task 05 — finalize / distill opt-ins. Off by default
        // on the inner handler; forwarded verbatim so unified-entry
        // callers can opt into the wave-17 finalization pass without
        // dropping back to a direct `mission_plan(action=execute)` call.
        // `distill_on_success` requires `finalize_plan=true`; the inner
        // handler's `validate_finalize_args` enforces that — we don't
        // re-validate at the pipeline layer.
        "finalize_plan",
        "distill_on_success",
        "distill_mode",
        // wave-18 / task 05 — cross-plan distill chain opt-ins. All
        // three knobs are forwarded together so the inner
        // `validate_distill_chain_args` can enforce the cross-field
        // rule (chain knobs require `finalize_plan=true`); we never
        // re-validate at the pipeline layer.
        "distill_chain_id",
        "distill_chain_mode",
        "distill_chain_name",
        // wave-18 / task 06 — autonomous PLAN field inference opt-in.
        // Default "off" on the inner handler. Strict allowlist
        // (off/preview/apply_safe) is enforced by
        // `parse_infer_plan_fields_mode` so a typo fails fast there
        // rather than silently degrading to "off" at the pipeline
        // layer.
        "infer_plan_fields",
        // wave-19 / task 06 — task-contract emitter knobs. Off by
        // default; forwarded verbatim so a unified-entry caller can
        // opt the workstation substrate into the wave-19 emitter
        // without dropping back to a direct mission_plan call. The
        // inner `parse_task_contract_emit_mode` enforces the
        // (off|emit|emit_dry_run) allowlist.
        "task_contract_mode",
        "emit_task_contract",
        // wave-20 / task 04 — machine-driven dispatch knobs. Default
        // `rendered` preserves the wave-15..19 byte-shape; `machine`
        // (or `render_markdown=false`) instructs the workstation
        // substrate to consume the emitted task.lisp directly.
        // Forwarded verbatim so the inner
        // `parse_dispatch_contract_mode` enforces the
        // (rendered|machine) allowlist.
        "dispatch_contract_mode",
        "render_markdown",
    ] {
        if let Some(v) = args.get(key) {
            if !v.is_null() {
                out.insert(key.into(), v.clone());
            }
        }
    }
    Value::Object(out)
}

/// Forward the wave-14 file-first SSOT writer args
/// (`write_file / topic / overwrite_file / project / cwd / target_project`)
/// from the unified entry input bag to a downstream compile call. Pure
/// pass-through: blank strings are dropped (`nonblank`) so the inner
/// handler sees the same "absent" semantics it would from a direct call;
/// boolean fields default to absent (let the inner handler pick its own
/// default of `false`).
///
/// We deliberately do NOT inject a `topic` / `project` default here —
/// the inner handlers carry the canonical defaulting policy
/// (directive: topic required; plan: topic falls back to `board_task_id`),
/// and silently filling either at the pipeline layer would mask schema
/// errors that should fail loudly.
fn forward_file_first_args(args: &Value, out: &mut serde_json::Map<String, Value>) {
    if let Some(b) = args.get("write_file").and_then(|v| v.as_bool()) {
        out.insert("write_file".into(), json!(b));
    }
    if let Some(b) = args.get("overwrite_file").and_then(|v| v.as_bool()) {
        out.insert("overwrite_file".into(), json!(b));
    }
    if let Some(t) = nonblank(args.get("topic")) {
        out.insert("topic".into(), json!(t));
    }
    if let Some(p) = nonblank(args.get("project")) {
        out.insert("project".into(), json!(p));
    }
    if let Some(c) = nonblank(args.get("cwd")) {
        out.insert("cwd".into(), json!(c));
    }
    if let Some(tp) = nonblank(args.get("target_project")) {
        out.insert("target_project".into(), json!(tp));
    }
}

/// Forward the wave-14 review-gate args
/// (`review_gate_policy / emit_review_question / review_question_text /
///  review_question_id`) from the unified entry input bag to a downstream
/// compile call.
///
/// `review_gate_policy` is forwarded verbatim (including unknown values) so
/// the inner `parse_review_gate_policy` can stamp the resolved policy on
/// the response — masking unknown values here would hide caller typos.
/// `emit_review_question` defaults to absent (let the inner handler pick
/// `false`); `review_question_text` / `review_question_id` are blank-filtered
/// so an empty string never overrides a derived id.
fn forward_review_gate_args(args: &Value, out: &mut serde_json::Map<String, Value>) {
    if let Some(p) = nonblank(args.get("review_gate_policy")) {
        out.insert("review_gate_policy".into(), json!(p));
    }
    if let Some(b) = args.get("emit_review_question").and_then(|v| v.as_bool()) {
        out.insert("emit_review_question".into(), json!(b));
    }
    if let Some(t) = nonblank(args.get("review_question_text")) {
        out.insert("review_question_text".into(), json!(t));
    }
    if let Some(id) = nonblank(args.get("review_question_id")) {
        out.insert("review_question_id".into(), json!(id));
    }
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
        DecorateContext {
            stage: stages::MESSAGE_INTAKE,
            scope: ArtifactScope::Directive,
            next_step:
                "review the compiled directive then re-call this pipeline with `approved_directive_id` \
                 (and the directive's `version`) once the human gate passes",
            next_call: Some(json!({
                "tool": "mission_directive",
                "action": "approve",
                "note":
                    "approve via mission_directive(action=approve, directive_id=…, version=…). \
                     v0/v1 unified entry does NOT auto-approve.",
            })),
            expects_next_inputs: json!({
                "approved_directive_id": "uuid",
                "directive_version": "i32",
                "board_task_id": "string",
            }),
        },
    ))
}

async fn run_plan_compile_stage(state: &AppState, compile_args: Value) -> Result<ToolResult> {
    let inner = super::plan::handle(state, "mission_plan", compile_args).await?;
    Ok(decorate(
        inner,
        DecorateContext {
            stage: stages::PLAN_AUTHORING,
            scope: ArtifactScope::Plan,
            next_step:
                "review the compiled PLAN.lisp then re-call this pipeline with `approved_plan_id` \
                 and `execute=true` once the human gate passes",
            next_call: Some(json!({
                "tool": "mission_plan",
                "action": "approve",
                "note":
                    "approve via mission_plan(action=approve, plan_id=…). \
                     v0/v1 unified entry does NOT auto-approve.",
            })),
            expects_next_inputs: json!({
                "approved_plan_id": "uuid",
                "execute": true,
            }),
        },
    ))
}

async fn run_plan_execute_stage(state: &AppState, execute_args: Value) -> Result<ToolResult> {
    let inner = super::plan::handle(state, "mission_plan", execute_args).await?;
    Ok(decorate(
        inner,
        DecorateContext {
            stage: stages::EXECUTION_RUNNER,
            scope: ArtifactScope::Execution,
            next_step:
                "execution dispatched; collect evidence via mission_plan(action=record_evidence) \
                 and (when running with auto_distill) mission_workflow(action=distill)",
            next_call: None,
            expects_next_inputs: json!({}),
        },
    ))
}

// ───────────────────────────────────────────────────────────────────────
// decorate — append unified-entry breadcrumbs (pipeline_stage, flow_ref,
// artifact_refs, next_step, next_call) to the inner handler's response.
//
// wave-14 / Task 04 :: artifact_refs is the new top-level surface — we
// project the inner handler's payload (file_* / review_question_* / id +
// version pointers) into a single flat object so callers can correlate
// without re-parsing the inner JSON. Legacy callers (no write_file, no
// review_gate_policy) see only the row-id pointers; we never fabricate a
// `file_written=false` for callers that didn't ask.
// ───────────────────────────────────────────────────────────────────────

/// Which scope label the decorator should stamp into `artifact_refs.scope`.
/// Matches the deterministic review-question id `<scope>` slot
/// (`derive_review_question_id_for_artifact`) so retro queries can join
/// `pipeline_stage` ↔ `review_question_id` without a translation table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArtifactScope {
    Directive,
    Plan,
    Execution,
}

impl ArtifactScope {
    fn as_str(self) -> &'static str {
        match self {
            ArtifactScope::Directive => "directive",
            ArtifactScope::Plan => "plan",
            ArtifactScope::Execution => "execution",
        }
    }
}

/// All metadata the decorator needs in one bag — keeps the per-stage
/// runners terse and stops a future `next_step` addition from forcing a
/// signature churn.
struct DecorateContext<'a> {
    stage: &'a str,
    scope: ArtifactScope,
    next_step: &'a str,
    next_call: Option<Value>,
    expects_next_inputs: Value,
}

/// Append unified-entry breadcrumbs to whatever the inner handler returned.
///
/// Important: when the inner handler itself returned a `structured_error`,
/// we keep `is_error=true` and *still* surface the stage labels so the
/// caller's pipeline-step UI doesn't lose context. The error payload from
/// the inner handler is the first content element; the meta payload is
/// appended as a sibling so introspection remains lossless.
fn decorate(mut inner: ToolResult, ctx: DecorateContext<'_>) -> ToolResult {
    // We don't reach into the inner ToolResult's structured fields — the
    // public ToolResult contract here is "JSON in the first content
    // element". We *read* that element to project artifact_refs, then
    // append a sibling content element carrying the pipeline metadata;
    // this guarantees zero behavioural change for callers that just want
    // the inner JSON.
    let inner_payload = first_content_as_json(&inner);
    let artifact_refs = build_artifact_refs(ctx.scope, &inner_payload);

    let mut meta = serde_json::Map::new();
    meta.insert("pipeline_stage".into(), json!(ctx.stage));
    meta.insert("flow_ref".into(), json!(FLOW_REF));
    meta.insert("artifact_refs".into(), artifact_refs);
    meta.insert("next_step".into(), json!(ctx.next_step));
    meta.insert("expects_next_inputs".into(), ctx.expects_next_inputs);
    if let Some(nc) = ctx.next_call {
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

    inner.content.push(missiond_mcp::tools::ToolContent::Text {
        text: serde_json::to_string_pretty(&Value::Object(meta))
            .unwrap_or_else(|_| "{}".to_string()),
    });
    inner
}

/// Best-effort projection of the inner handler's first JSON content into a
/// `Value`. When the inner element is not parsable JSON (e.g. structured
/// error path that already serialises to a JSON string), we still return a
/// `Value::Null` so the decorator can keep going — `build_artifact_refs`
/// gracefully degrades when the input is `Null` or an empty object.
fn first_content_as_json(result: &ToolResult) -> Value {
    match result.content.first() {
        Some(missiond_mcp::tools::ToolContent::Text { text }) => {
            serde_json::from_str::<Value>(text).unwrap_or(Value::Null)
        }
        None => Value::Null,
    }
}

/// Build the `artifact_refs` object surfaced by every v1 response.
///
/// Layout (omitted keys never appear → easy `serde_json::from_value` round
/// trip + cheap-to-grep responses):
///
/// ```text
/// {
///   "scope":               "directive" | "plan" | "execution",
///   // Row-id pointers (always when the inner handler stamped them):
///   "directive_id":        "uuid",
///   "plan_id":             "uuid",
///   "version":             1,
///   "board_task_id":       "btk-…",
///   "status":              "compiled" | "partial" | …,
///   // file-first SSOT splice (only when write_file=true was forwarded):
///   "file_written":        true,
///   "file_path":           "<project_root>/.missiond/…/<artifact>.lisp",
///   "file_sha256":         "<hex>",
///   "file_bytes":          1234,
///   "file_created":        true,
///   "file_overwritten":    false,
///   "file_write_error":    "<reason>",          // partial path only
///   // review-gate splice (only when review_gate_policy ≠ Manual or the
///   // legacy `emit_review_question=true` opt-in fired):
///   "review_gate_policy":      "manual" | "emit_question" | "off",
///   "review_question_emitted": true,
///   "review_question_id":      "review:<scope>:<id>:v<v>:compile:<topic-hash>",
///   "review_question_text":    "<echoed text>",
///   "review_question_warning": { "code": …, "reason": …, … }
/// }
/// ```
///
/// Pure projection — no IO, no derivation. The inner handler is the SSOT
/// for whether a field exists; we just *lift* it. Legacy callers that omit
/// every wave-14 opt-in see only `scope` + the row-id pointers.
fn build_artifact_refs(scope: ArtifactScope, payload: &Value) -> Value {
    let mut refs = serde_json::Map::new();
    refs.insert("scope".into(), json!(scope.as_str()));
    let map = match payload.as_object() {
        Some(m) => m,
        None => return Value::Object(refs),
    };

    // Row-id pointers — wave-13 surfaces these on every persist=true compile
    // and on every successful execute. We surface whichever subset the inner
    // payload carries (directive emits `directive_id`; plan emits `plan_id`
    // + `board_task_id`; execute emits `plan_id` + `board_task_id`).
    for key in [
        "directive_id",
        "plan_id",
        "version",
        "board_task_id",
        "status",
        "compiler_mode",
        "compiler_model",
    ] {
        if let Some(v) = map.get(key) {
            if !v.is_null() {
                refs.insert(key.into(), v.clone());
            }
        }
    }

    // File-first SSOT splice — present only when the inner handler ran the
    // wave-14 writer. We never fabricate `file_written=false` for callers
    // that didn't opt in (write_file=false → no file_* fields anywhere).
    for key in [
        "file_written",
        "file_path",
        "file_sha256",
        "file_bytes",
        "file_created",
        "file_overwritten",
        "file_write_error",
    ] {
        if let Some(v) = map.get(key) {
            if !v.is_null() {
                refs.insert(key.into(), v.clone());
            }
        }
    }

    // Review-gate splice — present only when the policy was non-default OR
    // the legacy `emit_review_question=true` bool fired.
    for key in [
        "review_gate_policy",
        "review_question_emitted",
        "review_question_id",
        "review_question_text",
        "review_question_warning",
    ] {
        if let Some(v) = map.get(key) {
            if !v.is_null() {
                refs.insert(key.into(), v.clone());
            }
        }
    }

    Value::Object(refs)
}

fn planner_error_response(err: PlannerError) -> ToolResult {
    // ToolError schema is `{error_code, reason, suggestion?, trace_id?}` —
    // no `context` slot today. We surface the pipeline-stage breadcrumb
    // by appending a sibling `ToolContent::Text` carrying the metadata,
    // mirroring how `decorate` augments successful responses.
    let tool_err = ToolError::new(err.code(), err.message().to_string())
        .with_suggestion(err.suggestion());
    let mut res = ToolResult::structured_error(tool_err);
    // wave-14 / Task 04 :: planner errors carry the same skeleton as a
    // successful decoration so consumer dashboards can rely on a single
    // shape. `artifact_refs` is just the scope marker — no rows / files
    // / review questions exist yet because the planner short-circuited
    // before any handler ran.
    let stage = planner_error_stage(&err);
    let scope = planner_error_scope(&err);
    let meta = json!({
        "pipeline_stage": stage,
        "flow_ref": FLOW_REF,
        "artifact_refs": { "scope": scope.as_str() },
        "next_step": err.suggestion(),
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

/// Map an early-fail planner error onto the same scope label the decorator
/// would have stamped if the matching stage had run. Keeps the
/// `artifact_refs.scope` field consistent across the success / error
/// branches so consumer dashboards can pivot on a single field.
fn planner_error_scope(err: &PlannerError) -> ArtifactScope {
    match err {
        PlannerError::MissingMessage => ArtifactScope::Directive,
        PlannerError::PlanCompileMissingBoardTask => ArtifactScope::Plan,
        PlannerError::ApprovedPlanWithoutExecuteFlag => ArtifactScope::Plan,
        PlannerError::ExecuteWithoutApprovedPlan => ArtifactScope::Execution,
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
        assert!(out.get("topic").is_none(), "blank topic must collapse to absent");
        assert!(out.get("project").is_none(), "blank project must collapse to absent");
        assert!(out.get("cwd").is_none(), "blank cwd must collapse to absent");
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
        assert_eq!(
            out["review_question_id"],
            "review:directive:abc:v1:compile"
        );
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
        assert_eq!(compile_args["utterance"], "ship file-first writer integration");
        assert_eq!(compile_args["persist"], true);
        // wave-14 v1 file-first forwarding.
        assert_eq!(compile_args["write_file"], true);
        assert_eq!(compile_args["overwrite_file"], false);
        assert_eq!(compile_args["topic"], "wave14-04");
        assert_eq!(compile_args["project"], "missiond");
        assert_eq!(compile_args["target_project"], "missiond");
        assert_eq!(
            compile_args["cwd"],
            "/Users/jinchen/Projects/missiond"
        );
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
        assert_eq!(compile_args["review_question_text"], "Confirm directive scope");
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
        // file-first.
        assert_eq!(compile_args["write_file"], true);
        assert_eq!(compile_args["overwrite_file"], true);
        assert_eq!(compile_args["topic"], "wave14-04-plan");
        assert_eq!(compile_args["project"], "missiond");
        assert_eq!(
            compile_args["cwd"],
            "/Users/jinchen/Projects/missiond"
        );
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
        assert_eq!(
            refs["directive_id"],
            "00000000-0000-0000-0000-000000000abc"
        );
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
        assert_eq!(
            refs["plan_id"],
            "11111111-1111-1111-1111-111111111111"
        );
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
        let meta: Value =
            serde_json::from_str(&meta_text).expect("meta must serialise as JSON");
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
                next_step:
                    "review the compiled directive then re-call this pipeline with `approved_directive_id`",
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
                next_step:
                    "review the compiled PLAN.lisp then re-call this pipeline with `approved_plan_id` and `execute=true`",
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
                next_step:
                    "execution dispatched (or would-dispatch in dry_run); collect evidence via mission_plan(action=record_evidence)",
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
        let plan_id_uuid = uuid::Uuid::parse_str("22222222-2222-2222-2222-000000000a17")
            .expect("valid plan uuid");
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
        let pause_review_qid =
            super::super::review_gate::derive_plan_node_review_question_id(
                &plan_id,
                plan_version,
                paused_node_id,
                Some("plan-node"),
            );
        // Layer 1: envelope shape.
        let parsed_qid =
            super::super::review_gate::parse_review_question_id_struct(&pause_review_qid)
                .expect("derived qid must parse");
        assert_eq!(parsed_qid.scope, "plan");
        assert_eq!(parsed_qid.action, "plan-node");
        assert_eq!(parsed_qid.artifact_id, plan_id);
        assert_eq!(parsed_qid.version, plan_version);
        assert_eq!(
            parsed_qid.topic_hash.as_deref(),
            Some(
                super::super::review_gate::derive_plan_node_topic_hash(paused_node_id)
                    .as_str()
            )
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
                next_step:
                    "DAG paused at review gate; resolve via mission_review then re-call this pipeline with `resume_review_question_id`",
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
        assert_eq!(s6a_meta["artifact_refs"]["file_sha256"], "deadbeefcafebabe1111");

        // Inner payload (preserved byte-for-byte by `decorate`) carries
        // the paused-node surface + finalization "no lie" block.
        let s6a_inner_text = match &s6a_decorated.content[0] {
            missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
        };
        let s6a_inner_payload: Value = serde_json::from_str(&s6a_inner_text)
            .expect("s6a inner payload parses");
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
            s6a_inner_payload["finalization"]["final_plan_status"],
            "succeeded",
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
        assert_eq!(s6b_meta["artifact_refs"]["file_sha256"], "deadbeefcafebabe2222");

        let s6b_inner_text = match &s6b_decorated.content[0] {
            missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
        };
        let s6b_inner_payload: Value = serde_json::from_str(&s6b_inner_text)
            .expect("s6b inner payload parses");
        // Aggregate flips to `dag_succeeded` and the previously-paused
        // node has reached the `succeeded` terminal state.
        assert_eq!(s6b_inner_payload["aggregate_status"], "dag_succeeded");
        assert_eq!(s6b_inner_payload["runner_status"], "all_nodes_dispatched");
        assert_eq!(
            s6b_inner_payload["node_results"][0]["state"],
            "succeeded",
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
            s6a_meta["artifact_refs"]["file_path"],
            s6b_meta["artifact_refs"]["file_path"],
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
        let plan_id_uuid = uuid::Uuid::parse_str("44444444-4444-4444-4444-000000000a18")
            .expect("valid plan uuid");
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
        let pause_review_qid =
            super::super::review_gate::derive_plan_node_review_question_id(
                &plan_id,
                plan_version,
                paused_node_id,
                Some("plan-node"),
            );
        let parsed_qid =
            super::super::review_gate::parse_review_question_id_struct(&pause_review_qid)
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
        let evidence_path =
            "/tmp/missiond-smoke/.missiond/v2/plans/wave18-task09.evidence.json";
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
        assert_eq!(s6a_meta["artifact_refs"]["file_sha256"], "deadbeefcafebabe1818");

        // Inner payload assertions — wave-18 features pinned here.
        let s6a_inner_text = match &s6a_decorated.content[0] {
            missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
        };
        let s6a_inner_payload: Value = serde_json::from_str(&s6a_inner_text)
            .expect("s6a inner payload parses");
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
        assert_eq!(s6a_inner_payload["distill_chain_status"], "skipped_plan_not_succeeded");
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
        assert_eq!(s6a_inner_payload["node_results"][0]["retry"]["max_attempts"], 2);
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
        assert_eq!(s6b_meta["artifact_refs"]["file_sha256"], "deadbeefcafebabe2828");

        let s6b_inner_text = match &s6b_decorated.content[0] {
            missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
        };
        let s6b_inner_payload: Value = serde_json::from_str(&s6b_inner_text)
            .expect("s6b inner payload parses");

        // ── Acceptance fan-in (wave-18 / task 03) ──────────────────────
        let gate_acceptance = &s6b_inner_payload["node_results"][1]["acceptance"];
        assert_eq!(gate_acceptance["status"], "accepted");
        assert_eq!(gate_acceptance["mode"], "inner_status");
        let fan_in = &gate_acceptance["fan_in"];
        assert_eq!(fan_in["mode"], "all-succeeded");
        assert_eq!(fan_in["source_nodes"][0], dep_node_id);
        assert_eq!(fan_in["passed"], true);
        assert!(fan_in["reason"]
            .as_str()
            .unwrap()
            .contains("succeeded"));

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
            s6a_meta["artifact_refs"]["file_path"],
            s6b_meta["artifact_refs"]["file_path"],
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
}
