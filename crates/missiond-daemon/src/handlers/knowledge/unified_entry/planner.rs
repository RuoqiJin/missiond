use missiond_mcp::tools::error_codes;
use serde_json::{json, Value};

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
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::MissingMessage => error_codes::MISSING_PARAM,
            Self::PlanCompileMissingBoardTask => error_codes::MISSING_PARAM,
            Self::ExecuteWithoutApprovedPlan => error_codes::MISSING_PARAM,
            Self::ApprovedPlanWithoutExecuteFlag => error_codes::INVALID_PARAM,
        }
    }

    pub(super) fn message(&self) -> &'static str {
        match self {
            Self::MissingMessage => {
                "unified entry pipeline requires a non-empty `message` to seed s1 message-intake"
            }
            Self::PlanCompileMissingBoardTask => {
                "advancing past s3 to s4 plan-authoring requires `board_task_id` (PLAN.lisp anchors against it; planner refuses to fabricate one)"
            }
            Self::ExecuteWithoutApprovedPlan => {
                "execute=true requested but no `approved_plan_id` provided; s6 execution-runner refuses to dispatch without an explicit approved plan id"
            }
            Self::ApprovedPlanWithoutExecuteFlag => {
                "`approved_plan_id` provided without `execute=true`; v0 unified entry does NOT auto-execute — set execute=true (or execute_after_approval=true) to dispatch s6"
            }
        }
    }

    pub(super) fn suggestion(&self) -> &'static str {
        match self {
            Self::MissingMessage => {
                "pass `message` (the user utterance / external request body) at minimum"
            }
            Self::PlanCompileMissingBoardTask => {
                "create a board task first via mission_board_create, then re-call with board_task_id"
            }
            Self::ExecuteWithoutApprovedPlan => {
                "complete s4+s5 first (mission_plan compile + approve), then re-call with approved_plan_id"
            }
            Self::ApprovedPlanWithoutExecuteFlag => {
                "re-call with execute=true to dispatch s6, or omit approved_plan_id to stop after s5 review pointer"
            }
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

pub(super) fn build_directive_compile_args(message: String, args: &Value) -> Value {
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
    forward_array(
        args,
        "directive_affected_pillars",
        &mut out,
        "affected_pillars",
    );
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

pub(super) fn build_plan_compile_args(
    approved_directive_id: String,
    board_task_id: String,
    args: &Value,
) -> Value {
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
    if let Some(t) = nonblank(args.get("target")) {
        out.insert("target".into(), json!(t));
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
    if let Some(o) = nonblank(args.get("objective")) {
        out.insert("objective".into(), json!(o));
    }
    if let Some(cwd) = nonblank(args.get("requested_cwd")) {
        out.insert("requested_cwd".into(), json!(cwd));
    }
    if let Some(flow_id) = nonblank(args.get("flow_id")) {
        out.insert("flow_id".into(), json!(flow_id));
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

pub(super) fn build_plan_execute_args(approved_plan_id: String, args: &Value) -> Value {
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
        // wave-20 / task 08 — review-question listener auto-answer
        // knob. Default `off` preserves the wave-15..19 byte-shape;
        // `deterministic_safe` MAY auto-answer Approved on the
        // wave-16/02 listener path when every safety rule passes AND
        // the action is non-destructive; `dry_run` computes the
        // deterministic outcome without ever mutating state. The
        // pipeline never re-validates — `parse_auto_answer_policy`
        // owns the strict allowlist (off|deterministic_safe|dry_run).
        // Live LLM auto-approval is forbidden (the policy is pure
        // deterministic). Destructive actions (archive/supersede/
        // remove) NEVER auto-promote even under deterministic_safe.
        "auto_answer_policy",
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
pub(super) fn forward_file_first_args(args: &Value, out: &mut serde_json::Map<String, Value>) {
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
pub(super) fn forward_review_gate_args(args: &Value, out: &mut serde_json::Map<String, Value>) {
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

pub(super) fn nonblank(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
