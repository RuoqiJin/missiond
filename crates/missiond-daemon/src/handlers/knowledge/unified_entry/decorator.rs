use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};

use super::planner::PlannerError;
use super::stages;

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
pub(super) struct DecorateContext<'a> {
    pub(super) stage: &'a str,
    pub(super) scope: ArtifactScope,
    pub(super) next_step: &'a str,
    pub(super) next_call: Option<Value>,
    pub(super) expects_next_inputs: Value,
}

/// Append unified-entry breadcrumbs to whatever the inner handler returned.
///
/// Important: when the inner handler itself returned a `structured_error`,
/// we keep `is_error=true` and *still* surface the stage labels so the
/// caller's pipeline-step UI doesn't lose context. The error payload from
/// the inner handler is the first content element; the meta payload is
/// appended as a sibling so introspection remains lossless.
pub(super) fn decorate(mut inner: ToolResult, ctx: DecorateContext<'_>) -> ToolResult {
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
    meta.insert("flow_ref".into(), json!(stages::FLOW_REF));
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
///   "review_question_warning": { "code": …, "reason": …, … },
///   // wave-19/06 + wave-20/04 machine-contract splice (only when the
///   // inner handler stamped one of these keys — wave-15..19 shape is
///   // preserved for callers that never opt in):
///   "task_contract_mode":          "emit" | "emit_dry_run" | "off",
///   "task_contract_eligible":      true,
///   "task_contract_path":          "<project_root>/.missiond/tasks/generated/…/root.lisp",
///   "task_contract_source_path":   "<same path the workstation consumed>",
///   "dispatch_contract_mode":      "rendered" | "machine",
///   "render_command":              "node scripts/render-claudecode-task.mjs <path>",
///   "task_contract_skip_reason":   "<reason node was ineligible>",
///   "task_contract_error":         "<reason emission failed>"
/// }
/// ```
///
/// Pure projection — no IO, no derivation. The inner handler is the SSOT
/// for whether a field exists; we just *lift* it. Legacy callers that omit
/// every wave-14 opt-in see only `scope` + the row-id pointers.
pub(super) fn build_artifact_refs(scope: ArtifactScope, payload: &Value) -> Value {
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

    // wave-19 / task 06 + wave-20 / task 04 — machine-contract splice.
    // Surfaced only when the inner handler stamped one of these keys (the
    // wave-15..19 byte-shape stays identical for callers that never opt
    // into `task_contract_mode` / `dispatch_contract_mode`). Lifting them
    // here means a single envelope shape covers the machine handoff:
    //   * `task_contract_mode`         — emit | emit_dry_run | off
    //   * `task_contract_eligible`     — bus the emitter judged the node on
    //   * `task_contract_path`         — on-disk Lisp written by the emitter
    //   * `task_contract_source_path`  — path the workstation consumer read
    //                                     (proves the Lisp drove the brief)
    //   * `dispatch_contract_mode`     — rendered (default) | machine
    //   * `render_command`             — optional compatibility metadata for
    //                                     out-of-process Markdown rendering
    //   * `task_contract_skip_reason`  — explains why eligibility failed
    //   * `task_contract_error`        — explains why emission failed
    //
    // wave-20 / task 05 — these fields are the proof that the machine
    // handoff is the SSOT: a caller observing the unified-entry envelope
    // can verify `dispatch_contract_mode == "machine"` AND
    // `task_contract_source_path == task_contract_path` without diving
    // back into the inner JSON payload.
    for key in [
        "task_contract_mode",
        "task_contract_eligible",
        "task_contract_path",
        "task_contract_source_path",
        "dispatch_contract_mode",
        "render_command",
        "task_contract_skip_reason",
        "task_contract_error",
    ] {
        if let Some(v) = map.get(key) {
            if !v.is_null() {
                refs.insert(key.into(), v.clone());
            }
        }
    }

    // wave-20 / task 08 — review auto-answer policy splice. Surfaced
    // only when the inner handler stamped one of these keys (default
    // `off` produces no fields under `Off`, so the wave-15..19
    // byte-shape stays identical for callers that never opt into
    // `auto_answer_policy`). Lifting the four invariant fields here
    // means a single envelope shape covers the listener-path policy
    // outcome:
    //   * `auto_answer_policy`     — resolved policy label
    //                                 (off|deterministic_safe|dry_run)
    //   * `policy_result`          — outcome status label
    //                                 (not_evaluated|auto_answered|
    //                                  skipped_rules_failed|
    //                                  skipped_destructive_action|
    //                                  dry_run_preview)
    //   * `selected_decision`      — `approved | needs_changes` (NEVER
    //                                  `rejected` — invariant I1)
    //   * `safety_rule_results`    — array of `code:detail` strings
    //                                  from the wave-18/07 inspector
    //                                  + the wave-20/08 destructive-
    //                                  action rule
    //   * `requires_human`         — `true` whenever the listener
    //                                  must defer to a human reviewer
    for key in [
        "auto_answer_policy",
        "policy_result",
        "selected_decision",
        "safety_rule_results",
        "requires_human",
    ] {
        if let Some(v) = map.get(key) {
            if !v.is_null() {
                refs.insert(key.into(), v.clone());
            }
        }
    }

    Value::Object(refs)
}

pub(super) fn planner_error_response(err: PlannerError) -> ToolResult {
    // ToolError schema is `{error_code, reason, suggestion?, trace_id?}` —
    // no `context` slot today. We surface the pipeline-stage breadcrumb
    // by appending a sibling `ToolContent::Text` carrying the metadata,
    // mirroring how `decorate` augments successful responses.
    let tool_err =
        ToolError::new(err.code(), err.message().to_string()).with_suggestion(err.suggestion());
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
        "flow_ref": stages::FLOW_REF,
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

pub(super) fn planner_error_stage(err: &PlannerError) -> &'static str {
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
pub(super) fn planner_error_scope(err: &PlannerError) -> ArtifactScope {
    match err {
        PlannerError::MissingMessage => ArtifactScope::Directive,
        PlannerError::PlanCompileMissingBoardTask => ArtifactScope::Plan,
        PlannerError::ApprovedPlanWithoutExecuteFlag => ArtifactScope::Plan,
        PlannerError::ExecuteWithoutApprovedPlan => ArtifactScope::Execution,
    }
}
