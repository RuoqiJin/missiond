use missiond_core::types::Plan;

use crate::handlers::knowledge::workstation_dispatch;
use crate::state::AppState;

use super::super::DagNode;
use super::{build_rollback_descriptor, RollbackEvaluation, RollbackPolicy, RollbackStatus};

/// wave-17 / task 04 — execute the conservative rollback pass for a
/// just-failed node. Pure async wrapper over the descriptor /
/// safety-check / optional-dispatch pipeline so the wave loop's
/// final-failure branch can call a single helper.
///
/// Behaviour matrix (matches the wave-17 / task 04 brief):
///   * No rollback hints OR `:rollback-policy "none"` →
///     `RollbackEvaluation { status: NotRequested, ... }` and the
///     scheduler skips the rollback evidence emit entirely.
///   * `:rollback-policy "descriptor"` → fully-populated descriptor
///     evaluation with `status=DescriptorReady`, no dispatch attempt.
///   * `:rollback-policy "workstation"` + safety check fails →
///     `status=Refused` with the failing condition spelled out, no
///     dispatch attempt. SafeDescriptor refusals from the substrate
///     also collapse to `Refused`.
///   * `:rollback-policy "workstation"` + safety check passes →
///     dispatch via `run_workstation_dispatch`. On success
///     `status=Dispatched` (with brief preview + inner payload). On
///     inner-handler error `status=Failed` with the error message on
///     the reason. SafeDescriptor refusals (which can still surface
///     even after the static safety check passes — e.g. resolver
///     reports a non-existent project root) become `Refused` so the
///     non-retryable refusal vocabulary stays consistent across all
///     workstation-substrate consumers.
pub(in crate::handlers::knowledge::plan_dag) async fn run_rollback(
    state: &AppState,
    plan: &Plan,
    node: &DagNode,
) -> RollbackEvaluation {
    let descriptor = build_rollback_descriptor(node);
    match descriptor.policy {
        RollbackPolicy::None => RollbackEvaluation {
            policy: RollbackPolicy::None,
            status: RollbackStatus::NotRequested,
            reason: if node.has_rollback_hints() {
                "rollback policy explicitly set to none; no rollback dispatch".to_string()
            } else {
                "no rollback hints declared".to_string()
            },
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        },
        RollbackPolicy::Descriptor => {
            // Build the descriptor brief locally so observers see the
            // same shape they would for a forward task brief, but
            // NEVER dispatch.
            let hints = descriptor.to_workstation_hints(node);
            let strategy = node.dispatch_strategy.as_deref().unwrap_or("unknown");
            let preview = if descriptor.objective.is_some() {
                Some(truncate_rollback_brief_preview(
                    &workstation_dispatch::build_task_brief(plan, &hints, strategy),
                ))
            } else {
                None
            };
            RollbackEvaluation {
                policy: RollbackPolicy::Descriptor,
                status: RollbackStatus::DescriptorReady,
                reason: "descriptor mode: rollback intent recorded; no dispatch performed"
                    .to_string(),
                objective: descriptor.objective.clone(),
                owned_files: descriptor.owned_files.clone(),
                acceptance_commands: descriptor.acceptance_commands.clone(),
                task_brief_preview: preview,
                task_brief_path: None,
                inner_payload: None,
                cascade: None,
            }
        }
        RollbackPolicy::Workstation => {
            // Run the static safety check first so a refusal here
            // never touches the substrate. SafeDescriptor refusals
            // are non-retryable per the wave-15 contract.
            if let Err(reason) = descriptor.safety_check_for_workstation(node) {
                return RollbackEvaluation {
                    policy: RollbackPolicy::Workstation,
                    status: RollbackStatus::Refused,
                    reason: format!("rollback workstation dispatch refused: {}", reason),
                    objective: descriptor.objective,
                    owned_files: descriptor.owned_files,
                    acceptance_commands: descriptor.acceptance_commands,
                    task_brief_preview: None,
                    task_brief_path: None,
                    inner_payload: None,
                    cascade: None,
                };
            }
            // Static safety passed — dispatch through the substrate.
            // The substrate may STILL refuse (e.g. cwd not absolute,
            // project registry miss); we map every SafeDescriptor
            // refusal back to `Refused` so the non-retryable
            // vocabulary stays consistent.
            let hints = descriptor.to_workstation_hints(node);
            let strategy = node.dispatch_strategy.as_deref().unwrap_or("unknown");
            let outcome = workstation_dispatch::run_workstation_dispatch(
                state,
                plan,
                "mission_task_delegate",
                strategy,
                hints,
                false,
            )
            .await;
            match outcome {
                workstation_dispatch::WorkstationDispatchOutcome::Dispatched {
                    task_brief,
                    task_brief_path,
                    inner_payload,
                    ..
                } => RollbackEvaluation {
                    policy: RollbackPolicy::Workstation,
                    status: RollbackStatus::Dispatched,
                    reason: "rollback workstation dispatch completed; inner handler returned Ok"
                        .to_string(),
                    objective: descriptor.objective,
                    owned_files: descriptor.owned_files,
                    acceptance_commands: descriptor.acceptance_commands,
                    task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                    task_brief_path,
                    inner_payload: Some(inner_payload),
                    cascade: None,
                },
                workstation_dispatch::WorkstationDispatchOutcome::DryRun { task_brief } => {
                    // The wave loop never asks for dry_run on rollback
                    // (we always pass dry_run=false above). Defensive:
                    // if a future caller flips the knob we surface as
                    // dispatched with no inner payload so observers
                    // don't see a missing variant.
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Dispatched,
                        reason: "rollback dispatched in dry_run mode (no real handler invoked)"
                            .to_string(),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                        task_brief_path: None,
                        inner_payload: None,
                        cascade: None,
                    }
                }
                workstation_dispatch::WorkstationDispatchOutcome::InnerError {
                    task_brief,
                    inner_payload,
                } => {
                    let detail = inner_payload
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rollback inner handler returned error")
                        .to_string();
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Failed,
                        reason: format!("rollback workstation dispatch failed: {}", detail),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                        task_brief_path: None,
                        inner_payload: Some(inner_payload),
                        cascade: None,
                    }
                }
                workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor {
                    reason,
                    task_brief,
                } => {
                    // Substrate-side safety refusal — collapse to
                    // Refused so the wave loop treats it as
                    // non-retryable (mirrors wave-15 / task 05).
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Refused,
                        reason: format!(
                            "rollback workstation dispatch refused (substrate): {}",
                            reason.detail()
                        ),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: task_brief
                            .as_deref()
                            .map(truncate_rollback_brief_preview),
                        task_brief_path: None,
                        inner_payload: None,
                        cascade: None,
                    }
                }
            }
        }
    }
}

/// wave-17 / task 04 — local copy of the workstation-dispatch preview
/// truncation so the rollback evaluation block surfaces a humane
/// preview without taking a dep on the substrate's private helper.
/// Same MAX (800 chars) so previews look identical across surfaces.
pub(in crate::handlers::knowledge::plan_dag) fn truncate_rollback_brief_preview(
    brief: &str,
) -> String {
    const MAX: usize = 800;
    if brief.len() <= MAX {
        return brief.to_string();
    }
    let mut end = MAX;
    while end > 0 && !brief.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &brief[..end])
}
