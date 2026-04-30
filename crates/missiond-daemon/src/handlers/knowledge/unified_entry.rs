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
//!                          + wave-20 / task 08 review auto-answer
//!                              policy opt-in (`auto_answer_policy`,
//!                              one of `off|deterministic_safe|dry_run`).
//!                              Default `off` preserves the wave-15..19
//!                              byte-shape; `deterministic_safe` MAY
//!                              auto-answer Approved on the wave-16/02
//!                              listener path when every safety rule
//!                              passes AND the action is
//!                              non-destructive (archive/supersede/
//!                              remove are NEVER promoted); `dry_run`
//!                              computes the deterministic outcome
//!                              without ever mutating state. The
//!                              policy NEVER auto-rejects (invariant
//!                              I1) and NEVER calls an LLM (invariant
//!                              I3) — pure deterministic safety
//!                              inspector. The wave-15..19 non-goal
//!                              "auto-answer a review question" stays
//!                              loud below; this v0 narrows it to
//!                              "live LLM auto-answer" while
//!                              permitting the deterministic
//!                              listener-side promotion under the
//!                              opt-in `deterministic_safe` mode.
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
//!     * auto-answer a review question  via LLM (the wave-20/08
//!                                       `auto_answer_policy` permits a
//!                                       deterministic listener-side
//!                                       Approved promotion under the
//!                                       opt-in `deterministic_safe`
//!                                       mode, but NEVER through an LLM
//!                                       and NEVER for destructive
//!                                       actions — see the wave-20/08
//!                                       I1+I2+I3 invariants on
//!                                       `evaluate_auto_answer_policy`)
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
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};

use crate::state::AppState;

mod decorator;
mod planner;
pub(crate) mod stages;

use decorator::{decorate, planner_error_response, ArtifactScope, DecorateContext};
use planner::{plan_pipeline, PipelineDecision};

#[cfg(test)]
use decorator::{build_artifact_refs, planner_error_scope, planner_error_stage};
#[cfg(test)]
use planner::{
    build_directive_compile_args, build_plan_compile_args, build_plan_execute_args,
    forward_file_first_args, forward_review_gate_args, nonblank, PlannerError,
};
#[cfg(test)]
use stages::FLOW_REF;

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
            next_step: "review the compiled directive then re-call this pipeline with `approved_directive_id` \
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
// tests — pure planner only (no AppState / DB / Sonnet).
//
// The async stage runners are not unit-tested here; they delegate to
// `directive::handle` / `plan::handle`, both of which already have
// extensive integration tests in their own modules. The unique value
// of this helper is the *routing* logic, which is fully covered below.
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
