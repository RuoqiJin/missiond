#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-plan-execution-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 plan.lisp execution isomorphism contract:
  - V3 blueprint declares plan.lisp as an executable routing artifact.
  - mission_plan compile dry_run renders routing hints into Lisp.
  - mission_plan execute can derive target/objective/dispatch hints from plan.sexp_text.
  - DAG execution parses node-local Lisp hints and forwards them to the same dispatch path.
  - unified_entry/planner.rs forwards plan compile/execute args instead of inventing a second plan schema.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  planHandler: 'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
  planTests: 'crates/missiond-daemon/src/handlers/knowledge/plan/tests.rs',
  planCompileAuthoring: 'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs',
  planApprovalReview: 'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs',
  planApprovalProposer:
    'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs',
  planApprovalSubscriber:
    'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs',
  planFieldInference: 'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference.rs',
  planFieldInferenceLlm: 'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/llm.rs',
  planFieldInferenceApply: 'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply.rs',
  planExecutionRuntime: 'crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs',
  planInternalDispatch: 'crates/missiond-daemon/src/handlers/knowledge/plan/internal_dispatch.rs',
  planExecuteHints: 'crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs',
  planTaskContract: 'crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs',
  planDistillChain: 'crates/missiond-daemon/src/handlers/knowledge/plan/distill_chain.rs',
  planDispatchResponse: 'crates/missiond-daemon/src/handlers/knowledge/plan/dispatch_response.rs',
  planEvidenceSidecar: 'crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs',
  planRouterPolicyAdapter: 'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs',
  planRouterPolicySchemaParser:
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs',
  planTaskRunnerAdapter: 'crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs',
  planDag: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs',
  planDagParser: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser.rs',
  planDagAcceptance: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance.rs',
  planDagClaimLease: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs',
  planDagDispatch: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch.rs',
  planDagRollback: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback.rs',
  planDagResume: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume.rs',
  planDagProjection: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/projection.rs',
  planDagFinalization: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/finalization.rs',
  planDagLifecycle: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle.rs',
  planDagScheduler: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/scheduler.rs',
  planDagMode: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/mode.rs',
  planDagTests: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/tests.rs',
  unifiedEntryPlanner: 'crates/missiond-daemon/src/handlers/knowledge/unified_entry/planner.rs',
  mcpPlan: 'crates/missiond-mcp/src/tools/knowledge/plan.rs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 plan execution Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(`v3 plan execution Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    '(surface mission_plan',
    ':status "code-aligned"',
    'compiler_mode=dry_run must still emit executable routing hints in Lisp',
    ':default-target mission_task_delegate',
    'plan artifact MUST be amended with :plan_id + :version + :board_task_id',
    'compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold',
    'plan/compile_authoring.rs owns mission_plan plan-authoring entry/core',
    'plan/approval_review.rs owns mission_plan plan-review-gate caller actions',
    'plan/approval_review/proposer.rs owns mission_plan plan-review LLM proposal helpers',
    'plan/approval_review/subscriber.rs owns mission_plan plan-review subscriber bridge',
    'plan/field_inference.rs owns mission_plan execute preflight field inference/core',
    'plan/field_inference/llm.rs owns Sonnet proposal parsing',
    'plan/field_inference/apply.rs owns apply_gate and persisted_apply',
    'plan/execution_runtime.rs owns mission_plan execute entry/core/egress orchestration',
    'plan/internal_dispatch.rs owns mission_plan inner target argument projection',
    'plan/execute_hints.rs owns mission_plan PLAN.lisp hint parsing',
    'plan/task_contract.rs owns mission_plan task-contract Lisp projection',
    'plan/distill_chain.rs owns mission_plan cross-plan distill-chain egress',
    'plan/dispatch_response.rs owns mission_plan execution response egress',
    'plan/evidence_sidecar.rs owns mission_plan evidence sidecar egress',
    'plan/router_policy_dry_run.rs owns the mission_plan router-policy adapter',
    'plan/router_policy_dry_run/schema_parser.rs owns the router-policy Lisp schema parser',
    'plan/task_runner_dry_run.rs owns the mission_plan task-runner adapter',
    'plan/tests.rs holds the historical mission_plan regression suite outside the runtime facade',
    'plan_dag/parser.rs owns the DAG parser/validator core',
    'plan_dag/dispatch.rs owns the DAG node dispatch bridge',
    'plan_dag/tests.rs does the same for the DAG scheduler regression suite',
    'plan_dag/rollback.rs owns the DAG rollback/cascade core',
    'plan_dag/resume.rs owns the DAG review-resume entry/egress core',
    'plan_dag/projection.rs owns the DAG response projection core',
    'plan_dag/finalization.rs owns the DAG finalization projection core',
    'plan_dag/lifecycle.rs owns the DAG lifecycle event/evidence projection core',
    'plan_dag/scheduler.rs owns the DAG scheduler projection core',
    'plan_dag/mode.rs owns the DAG scheduler-mode gate',
    'execute can derive target_source=plan_hint from plan.sexp_text',
    'DAG execution parses node-local Lisp hints',
    'node scripts/check-v3-plan-execution-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.planHandler, sources.planHandler, [
    'mod compile_authoring',
    'use compile_authoring::{action_compile, collect_string_list}',
    'mod execution_runtime',
    'use execution_runtime::action_execute',
    'mod internal_dispatch',
    'pub(super) use internal_dispatch::{build_internal_dispatch_args, tool_result_payload}',
    '#[cfg(test)]',
    'mod tests;',
  ]);

  requireAll(diagnostics, files.planTests, sources.planTests, [
    'use super::*;',
    'fn sha256_hex_is_64_chars',
    'fn require_str_rejects_empty',
    'fn dry_run_plan_sexp_carries_executable_target_hints',
    'fn build_internal_args_for_mission_execution_uses_plan_hints',
  ]);

  requireAll(diagnostics, files.planExecutionRuntime, sources.planExecutionRuntime, [
    'pub(super) async fn action_execute',
    'pub(super) fn action_execute_bridge',
    'pub(super) async fn action_execute_internal',
    'parse_plan_hints(&plan.sexp_text)',
    '(t, "plan_hint")',
    'target_source',
    'dispatch_strategy_source',
    'super::super::plan_dag::action_execute_dag_v1',
    'super::super::workstation_dispatch::run_workstation_dispatch_with_contract_and_trace',
    'pub(super) async fn compute_workstation_proposal_bundle',
    'pub(super) async fn compute_workstation_auto_spawn_gate',
    'pub(super) fn attach_inference_block',
  ]);

  requireAll(diagnostics, files.planInternalDispatch, sources.planInternalDispatch, [
    'pub(in crate::handlers::knowledge) fn build_internal_dispatch_args',
    '"mission_execution"',
    '"mission_task_delegate"',
    '"mission_flow_run"',
    'DERIVED_OBJECTIVE_MAX',
    'VALID_DELEGATE_INTENTS',
    'AGENT_TEAM_OBJECTIVE_HINT',
    'pub(super) fn derive_objective_from_plan',
    'pub(super) fn truncate_chars',
    'pub(in crate::handlers::knowledge) fn tool_result_payload',
  ]);

  requireAll(diagnostics, files.planCompileAuthoring, sources.planCompileAuthoring, [
    'pub(super) async fn action_compile',
    'fn resolve_dry_run_plan_target',
    'return Ok("mission_task_delegate");',
    'fn render_dry_run_plan_sexp',
    'String::from("(plan-draft\\n")',
    ':execution-readiness :dry-run-executable-scaffold',
    'push_lisp_string_field(&mut out, "target", input.target);',
    'push_lisp_string_field(&mut out, "objective", input.objective);',
    'out.push_str("  :nodes\\n");',
    'pub(super) fn build_planner_system_prompt',
    'pub(super) fn build_planner_user_prompt',
    'pub(super) fn validate_compiled_plan_sexp',
    'pub(super) async fn maybe_write_plan_artifact',
    'attempt_artifact_write',
  ]);

  requireAll(diagnostics, files.planApprovalReview, sources.planApprovalReview, [
    'mod proposer;',
    'mod subscriber;',
    'use self::proposer::{',
    'request_plan_auto_approve_proposal',
    'pub(crate) use self::subscriber::{handle_review_resolved_event, PlanSubscriberOutcome};',
    'pub(super) const PLAN_REVIEW_ACTIONS',
    'pub(super) async fn action_approve',
    'pub(super) async fn action_mark',
    'pub(super) async fn action_supersede',
  ]);

  requireAll(diagnostics, files.planApprovalProposer, sources.planApprovalProposer, [
    'use super::*;',
    'pub(super) fn build_plan_automation_ctx',
    'pub(super) async fn request_plan_auto_approve_proposal',
    'fn attach_plan_proposal_block',
    'pub(super) fn attach_plan_apply_gate_block',
    'pub(super) fn parse_plan_proposer_mode_or_error',
    'pub(super) fn plan_proposer_summary',
    'PLAN_REVIEW_PROPOSER_CALLER',
    'SONNET_PLAN_PROPOSER_MAX_TOKENS',
  ]);

  requireAll(diagnostics, files.planApprovalSubscriber, sources.planApprovalSubscriber, [
    'use super::*;',
    'pub(crate) enum PlanSubscriberOutcome',
    'pub(crate) async fn handle_review_resolved_event',
    'validate_review_resolution_envelope',
    'PLAN_REVIEW_ACTIONS',
    'PlanStatus::Approved',
    'MarkNeedsExplicitCall',
    'SupersedeNeedsExplicitCall',
  ]);

  requireAll(diagnostics, files.planFieldInference, sources.planFieldInference, [
    'pub(crate) enum InferPlanFieldsMode',
    'pub(crate) fn parse_infer_plan_fields_mode',
    'pub(super) struct PlanFieldInference',
    'pub(super) fn compute_plan_field_inference',
    'mod llm;',
    'pub(super) use llm::*;',
    'mod apply;',
    'pub(super) use apply::*;',
    'pub(super) const WORKSTATION_INFER_MODE_SONNET_SUGGEST',
    'pub(super) fn parse_workstation_inference_mode',
    'pub(super) fn refuse_workstation_inference_in_dag_mode',
    'plan_field_inference',
    'persisted_apply',
  ]);

  requireAll(diagnostics, files.planFieldInferenceLlm, sources.planFieldInferenceLlm, [
    'const LLM_ALLOWED_FIELDS',
    'struct LlmProposal',
    'struct LlmProposalBundle',
    'fn parse_llm_proposals',
    'fn reconcile_llm_conflicts',
    'fn build_llm_inference_prompt',
    'async fn request_llm_proposals',
    'fn deterministic_covers_all_fields',
    'async fn read_recent_evidence_entries',
  ]);

  requireAll(diagnostics, files.planFieldInferenceApply, sources.planFieldInferenceApply, [
    'fn apply_safe_augmentation',
    'enum ApplyOrigin',
    'struct ApplyGateOutcome',
    'fn validate_apply_gate_args',
    'fn compute_apply_gate',
    'enum PersistedApplyStatus',
    'struct PersistedApplyOutcome',
    'fn enforce_persisted_apply_preflight',
    'async fn execute_persisted_apply',
    'fn attach_persisted_apply_block',
  ]);

  requireAll(diagnostics, files.planExecuteHints, sources.planExecuteHints, [
    'pub(crate) struct ParsedPlanHints',
    'pub(crate) struct ResolvedExec',
    'pub(crate) fn parse_plan_hints',
    '"target" | "target-tool" | "tool"',
    '"objective" => store_first(&mut h.objective, &value)',
    'pub(crate) fn scan_keyword_pairs',
    'pub(crate) fn normalize_target',
    'pub(crate) fn canonicalize_strategy',
    'pub(crate) fn resolve_dispatch_strategy',
    'AGENT_TEAM_OBJECTIVE_HINT',
  ]);

  requireAll(diagnostics, files.planTaskContract, sources.planTaskContract, [
    'pub(crate) enum TaskContractEmitMode',
    'pub(crate) enum DispatchContractMode',
    'pub(crate) struct TaskContractInputs',
    'pub(crate) fn parse_task_contract_emit_mode',
    'pub(crate) fn parse_dispatch_contract_mode',
    'pub(crate) fn build_task_contract_lisp',
    'pub(crate) fn write_task_contract_under_root',
    'pub(crate) async fn emit_task_contract',
    'pub(crate) fn task_contract_inputs_from_hints',
    'pub(crate) fn task_contract_inputs_from_hints_with_trace',
    ':session-trace-path',
  ]);

  requireAll(diagnostics, files.planDistillChain, sources.planDistillChain, [
    'pub(super) const DISTILL_CHAIN_MODE_RECORD_ONLY',
    'pub(super) const CHAIN_RECORD_KIND',
    'pub(super) fn parse_distill_chain_id',
    'pub(super) fn parse_distill_chain_name',
    'pub(super) fn parse_distill_chain_mode',
    'pub(super) fn distill_chain_requested',
    'pub(super) fn validate_distill_chain_args',
    'pub(super) fn build_distill_chain_block',
    'pub(super) async fn apply_distill_chain',
    'pub(super) fn attach_distill_chain_to_payload',
    'distill_chain_status',
    'mission_workflow',
  ]);

  requireAll(diagnostics, files.planDispatchResponse, sources.planDispatchResponse, [
    'pub(super) fn validate_session_trace_path_arg',
    'fn reject_or_warn_trace_path',
    'pub(super) fn attach_session_trace_response_fields',
    'pub(crate) fn merge_task_contract_block',
    'pub(super) fn build_task_contract_failure_response',
    'pub(super) fn build_task_contract_dry_run_response',
    'pub(super) fn build_workstation_dispatch_response',
    'pub(super) fn build_internal_dispatch_success_response',
    'session_trace_path',
    'task_contract_emit_failed',
    'workstation_dispatch_v0',
    'status_update_failed',
  ]);

  requireAll(diagnostics, files.planEvidenceSidecar, sources.planEvidenceSidecar, [
    'pub(super) async fn action_record_evidence',
    'pub(crate) async fn append_plan_evidence_entry',
    'wrap_legacy_record_evidence',
    'recorded_at',
    'entry_count',
    'COMPANION_DIR',
    'source_override',
  ]);

  requireAll(diagnostics, files.planRouterPolicyAdapter, sources.planRouterPolicyAdapter, [
    'mod schema_parser',
    'schema_parser::parse_backend_registry(input)',
    'schema_parser::parse_router_policy(input)',
    'pub(super) enum RouterPolicyMode',
    'pub(super) fn parse_router_policy_mode',
    'pub(super) fn attach_router_recommendation_block',
    'fn compute_recommendation_block',
    'router_apply_eligible',
    'router_dispatch_descriptor',
    'backend_readiness_status',
    'Value::Bool(false)',
  ]);

  requireAll(
    diagnostics,
    files.planRouterPolicySchemaParser,
    sources.planRouterPolicySchemaParser,
    [
      'pub(super) fn parse_router_policy',
      'pub(super) fn parse_backend_registry',
      'const READINESS_STATUSES',
      'fn parse_rule',
      'fn parse_backend_entry',
      'fn parse_clause',
      'enum Sexp',
      'enum Token',
      'struct TokenCursor',
    ],
  );

  requireAll(diagnostics, files.planTaskRunnerAdapter, sources.planTaskRunnerAdapter, [
    'pub(super) enum TaskRunnerMode',
    'pub(super) fn parse_task_runner_mode',
    'pub(super) fn attach_task_runner_block',
    'fn build_runner_response_block',
    'Value::Bool(false)',
    'manifest_status',
    'overlap_diagnostics',
    'critical_path_minutes',
    'verification_tier_counts',
  ]);

  requireAll(diagnostics, files.planDag, sources.planDag, [
    'mod acceptance;',
    'use acceptance::{',
    'mod claim_lease;',
    'use claim_lease::{',
    'mod rollback;',
    'use rollback::{',
    'mod resume;',
    'pub(super) use resume::action_execute_resume;',
    'pub(super) use resume::validate_resume_request;',
    'pub(crate) use resume::{handle_review_resolved_plan_node_event, PlanNodeResumeListenerOutcome};',
    'mod parser;',
    'pub(super) use parser::{DagNode, ParsedDag};',
    'use parser::{build_validated_dag, ReviewGateKind, FAILURE_POLICY_FAIL_FAST};',
    'mod projection;',
    'use projection::{build_node_hint_summary, build_nodes_summary, build_retry_plan};',
    'mod scheduler;',
    'use scheduler::{',
    'mod dispatch;',
    'use dispatch::{dispatch_node, DispatchOutcome, TaskContractDispatchCtx};',
    'mod mode;',
    'pub(super) use mode::{detect_scheduler_mode, refuse_llm_inference_in_dag_mode};',
    'mod finalization;',
    'pub(super) use finalization::parse_finalize_plan;',
    'use finalization::{',
    'mod lifecycle;',
    'use lifecycle::{',
    '#[cfg(test)]',
    'mod tests;',
  ]);

  requireAll(diagnostics, files.planDagParser, sources.planDagParser, [
    'pub(in crate::handlers::knowledge) struct DagNode',
    'pub(super) enum ReviewGateKind',
    'pub(in crate::handlers::knowledge) struct ParsedDag',
    'pub(super) enum DagBuildError',
    'pub(super) fn build_validated_dag',
    'pub(super) fn parse_plan_dag',
    'fn scan_top_level_forms',
    'fn parse_node_form',
    '"target" | "target-tool" | "tool"',
    '"objective" => set_first(&mut objective, &value)',
    '"timeout-ms" | "timeout_ms"',
    '"target-project" | "target_project" | "project"',
    '"requested-cwd" | "requested_cwd" | "cwd"',
    '"acceptance-commands" | "acceptance_commands"',
    '"workstation-dispatch" | "workstation_dispatch"',
    'fn scan_keyword_pairs',
    'fn kahn_topo_sort',
    'pub(super) const MAX_NODE_ATTEMPTS_CAP',
    'pub(super) const MAX_RETRY_DELAY_MS',
    'pub(super) fn acceptance_mode_kind',
    'pub(super) fn has_acceptance_fan_in',
    'AcceptanceRequires::parse',
    'RollbackCascadeMode::parse',
  ]);

  requireAll(diagnostics, files.planDagAcceptance, sources.planDagAcceptance, [
    'pub(super) enum AcceptanceMode',
    'pub(super) enum AcceptanceRequires',
    'pub(super) enum AcceptanceStatus',
    'pub(super) struct AcceptanceEvaluation',
    'pub(super) struct AcceptanceFanInOutcome',
    'pub(super) fn evaluate_node_acceptance',
    'pub(super) fn apply_acceptance_fan_in',
    'fn inner_payload_failure_signal',
    'fn inner_payload_missing_keys',
    'pub(super) fn derive_acceptance_pause_id',
  ]);

  requireAll(diagnostics, files.planDagClaimLease, sources.planDagClaimLease, [
    'pub(super) const PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS',
    'pub(super) fn parse_claim_lease_secs',
    'pub(super) fn parse_claimer_name',
    'pub(super) fn parse_enforce_claims',
    'pub(super) fn derive_node_claim_scopes',
    'pub(super) fn derive_plan_dag_claim_id',
    'pub(super) struct PlanDagClaim',
    'pub(super) enum ClaimAcquire',
    'pub(super) struct ClaimRegistry',
    'pub(super) fn build_planned_claims',
    'scopes_overlap_pure',
  ]);

  requireAll(diagnostics, files.planDagDispatch, sources.planDagDispatch, [
    'pub(super) struct DispatchOutcome',
    'pub(super) struct TaskContractDispatchCtx',
    'pub(super) async fn dispatch_node',
    'fn node_to_workstation_hints',
    'fn workstation_outcome_to_dispatch_pair',
    'workstation_dispatch::run_workstation_dispatch_with_contract',
    'agent_execution::handle',
    'task_delegate::handle',
    'flow_run::handle',
    'plan::emit_task_contract',
    'plan::merge_task_contract_block',
    'tool_result_payload',
  ]);

  requireAll(diagnostics, files.planDagRollback, sources.planDagRollback, [
    'impl DagNode',
    'pub(super) enum RollbackPolicy',
    'pub(super) enum RollbackStatus',
    'pub(super) struct RollbackEvaluation',
    'pub(super) enum RollbackCascadeMode',
    'pub(super) struct CascadeCompensationOutcome',
    'pub(super) struct CascadeRollbackOutcome',
    'pub(super) fn build_rollback_descriptor',
    'pub(super) fn pre_dispatch_rollback_decision',
    'pub(super) async fn run_rollback',
    'pub(super) fn compute_compensation_order',
    'pub(super) fn build_compensation_plan_entry',
    'pub(super) async fn run_cascade_rollback',
    'fn map_dispatch_outcome_to_compensation',
    'fn truncate_rollback_brief_preview',
  ]);

  requireAll(diagnostics, files.planDagResume, sources.planDagResume, [
    'pub(in crate::handlers::knowledge) enum PlanNodeResumeError',
    'pub(in crate::handlers::knowledge) fn validate_resume_request',
    'pub(in crate::handlers::knowledge) async fn action_execute_resume',
    'fn resume_error_to_tool_result',
    'async fn emit_resume_decision_evidence',
    'pub(crate) enum PlanNodeResumeListenerOutcome',
    'pub(crate) async fn handle_review_resolved_plan_node_event',
    'parse_review_question_id_struct',
    'PlanNodeResumeInput',
    'TaskContractDispatchCtx::off()',
  ]);

  requireAll(diagnostics, files.planDagProjection, sources.planDagProjection, [
    'pub(super) fn build_nodes_summary',
    'pub(super) fn build_retry_plan',
    'pub(super) fn build_node_hint_summary',
    'RollbackPolicy::None',
    'unsupported_top_forms',
    'unsupported_fields',
  ]);

  requireAll(diagnostics, files.planDagFinalization, sources.planDagFinalization, [
    'pub(super) const FINALIZE_DISTILL_MODE_DRY_RUN',
    'pub(super) const FINALIZE_DISTILL_MODE_SONNET',
    'pub(in crate::handlers::knowledge) fn parse_finalize_plan',
    'pub(super) fn parse_distill_on_success',
    'pub(super) fn parse_distill_mode_arg',
    'pub(super) fn validate_finalize_args',
    'pub(super) fn finalize_plan_status_label',
    'fn unchanged_status_label',
    'pub(super) fn build_finalization_block',
    'pub(super) fn build_distill_block',
  ]);

  requireAll(diagnostics, files.planDagLifecycle, sources.planDagLifecycle, [
    'pub(super) struct EvidenceCtx',
    'pub(super) async fn emit_evidence_dag_finalized',
    'pub(super) fn deterministic_plan_node_event_id',
    'pub(super) fn build_plan_node_state_changed_event',
    'pub(super) async fn publish_plan_node_state_change',
    'pub(super) fn plan_node_should_retry',
    'pub(super) async fn emit_evidence_running',
    'pub(super) async fn emit_evidence_finished',
    'pub(super) async fn emit_evidence_rollback',
    'pub(super) async fn emit_evidence_acceptance',
    'pub(super) async fn emit_evidence_skipped',
    'pub(super) async fn emit_paused_review_gate',
    'pub(super) async fn emit_evidence_claimed',
    'pub(super) async fn emit_evidence_claim_released',
    'pub(super) async fn emit_evidence_claim_conflict',
    'with_state_transition("dag_finalized")',
    'publish_question(ev)',
    'EventRef::new(',
  ]);

  requireAll(diagnostics, files.planDagScheduler, sources.planDagScheduler, [
    'pub(super) fn compute_concurrency_plan',
    'pub(super) fn parse_max_parallel_nodes',
    'pub(super) fn propagate_taint',
    'pub(super) struct NodeInnerArgs',
    'pub(super) fn build_node_inner_args',
    'node_args.insert("timeout_secs".to_string()',
    'build_internal_dispatch_args',
    'ParsedPlanHints::default()',
  ]);

  requireAll(diagnostics, files.planDagMode, sources.planDagMode, [
    'pub(in crate::handlers::knowledge) fn detect_scheduler_mode',
    'pub(in crate::handlers::knowledge) fn refuse_llm_inference_in_dag_mode',
    'parse_infer_plan_fields_mode',
    'is_llm_augmented()',
    'infer_plan_fields=`{}` is single-node-execute-only in v0',
  ]);

  requireAll(diagnostics, files.planDagTests, sources.planDagTests, [
    'use super::*;',
    'use super::acceptance::*;',
    'use super::claim_lease::*;',
    'use super::finalization::*;',
    'use super::lifecycle::*;',
    'use super::mode::*;',
    'use super::parser::*;',
    'use super::projection::*;',
    'use super::resume::*;',
    'use super::rollback::*;',
    'use super::scheduler::*;',
    'fn parse_plan_dag_extracts_explicit_node_forms',
    'fn build_validated_dag_accepts_valid_chain',
    'fn validate_resume_request_routes_unique_paused_node',
    'fn claim_registry_rejects_overlapping_scope',
    'fn task_contract_dispatch_ctx_captures_machine_mode_for_dag',
  ]);

  requireAll(diagnostics, files.unifiedEntryPlanner, sources.unifiedEntryPlanner, [
    'pub(crate) fn plan_pipeline',
    'PipelineDecision::PlanCompile',
    'PipelineDecision::PlanExecute',
    'build_plan_compile_args',
    'build_plan_execute_args',
    '"compiler_mode"',
    '"target"',
    '"dispatch_strategy"',
    '"target_project"',
    '"objective"',
    '"requested_cwd"',
    '"flow_id"',
    '"timeout_secs"',
    '"infer_plan_fields"',
    'approved_plan_id',
    'execute_flag',
  ]);

  requireAll(diagnostics, files.mcpPlan, sources.mcpPlan, [
    '[compile dry_run | execute] compile dry_run renders this into PLAN.lisp as :target',
    'runner scans plan.sexp_text for :target / :target-tool / :tool hints',
    'Source-resolution precedence is explicit_arg > plan_hint > missing',
    '[execute internal mission_task_delegate] override the auto-derived objective',
    '[execute internal mission_task_delegate] passthrough timeout',
    'supported per-node fields: id / target / objective / depends-on / condition / failure-policy / timeout-ms / dispatch-strategy / target-project / requested-cwd / flow-id',
    'Declaring `:acceptance-commands` without a typed `:acceptance-mode` defaults to `manual_required`',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    requireText(diagnostics, file, source, needle);
  }
}

function requireText(diagnostics, file, source, needle) {
  if (!source.includes(needle)) {
    diagnostics.push({ file, message: `missing required contract text: ${needle}` });
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-plan-execution-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (artifact-contracts
    (artifact plan
      :runtime-hints (:default-target mission_task_delegate
        :rule "compiler_mode=dry_run must still emit executable routing hints in Lisp")
      :materialization-rule "plan artifact MUST be amended with :plan_id + :version + :board_task_id"))
  (implementation-map
    (surface mission_plan
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/llm.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/internal_dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/distill_chain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/dispatch_response.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/projection.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/finalization.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/scheduler.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/mode.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/tests.rs"]
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold; plan/compile_authoring.rs owns mission_plan plan-authoring entry/core; plan/approval_review.rs owns mission_plan plan-review-gate caller actions: action_approve, action_mark, action_supersede, review_automation_policy handling, and maybe_emit_review_question_resolved. plan/approval_review/proposer.rs owns mission_plan plan-review LLM proposal helpers: build_plan_automation_ctx, request_plan_auto_approve_proposal, attach_plan_proposal_block, attach_plan_apply_gate_block, parse_plan_proposer_mode_or_error, and plan_proposer_summary keep propose-only audit blocks outside the caller action facade. plan/approval_review/subscriber.rs owns mission_plan plan-review subscriber bridge: PlanSubscriberOutcome and handle_review_resolved_event keep approval/rejection/needs_changes transitions tied to the same review envelope validation without bloating the caller action facade. plan/field_inference.rs owns mission_plan execute preflight field inference/core; plan/field_inference/llm.rs owns Sonnet proposal parsing, validation, conflict reconciliation, prompt construction, gateway request, and recent evidence reads for inference; plan/field_inference/apply.rs owns apply_gate and persisted_apply, including explicit apply approval, LLM caller approval, proposal-hash preflight, PLAN.lisp persisted annotation synthesis, evidence entry construction, and response block splicing; plan/execution_runtime.rs owns mission_plan execute entry/core/egress orchestration; plan/internal_dispatch.rs owns mission_plan inner target argument projection; plan/execute_hints.rs owns mission_plan PLAN.lisp hint parsing; plan/task_contract.rs owns mission_plan task-contract Lisp projection; plan/distill_chain.rs owns mission_plan cross-plan distill-chain egress; plan/dispatch_response.rs owns mission_plan execution response egress; plan/evidence_sidecar.rs owns mission_plan evidence sidecar egress; plan/router_policy_dry_run.rs owns the mission_plan router-policy adapter; plan/router_policy_dry_run/schema_parser.rs owns the router-policy Lisp schema parser shared by the policy and backend-registry advisory projections; plan/task_runner_dry_run.rs owns the mission_plan task-runner adapter; plan/tests.rs holds the historical mission_plan regression suite outside the runtime facade; plan_dag/parser.rs owns the DAG parser/validator core; plan_dag/acceptance.rs owns the DAG acceptance core; plan_dag/claim_lease.rs owns the DAG claim/lease core; plan_dag/dispatch.rs owns the DAG node dispatch bridge into workstation-dispatch, task-contract emission, and internal handler execution; plan_dag/rollback.rs owns the DAG rollback/cascade core; plan_dag/resume.rs owns the DAG review-resume entry/egress core; plan_dag/projection.rs owns the DAG response projection core; plan_dag/finalization.rs owns the DAG finalization projection core; plan_dag/lifecycle.rs owns the DAG lifecycle event/evidence projection core; plan_dag/scheduler.rs owns the DAG scheduler projection core; plan_dag/mode.rs owns the DAG scheduler-mode gate; plan_dag/tests.rs does the same for the DAG scheduler regression suite; execute can derive target_source=plan_hint from plan.sexp_text. DAG execution parses node-local Lisp hints."))
  (compression-contract
    :checks ["node scripts/check-v3-plan-execution-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.planHandler, `
mod compile_authoring;
use compile_authoring::{action_compile, collect_string_list};
mod execution_runtime;
use execution_runtime::action_execute;
mod internal_dispatch;
pub(super) use internal_dispatch::{build_internal_dispatch_args, tool_result_payload};
#[cfg(test)]
mod tests;
`);
  writeFixture(root, DEFAULT_FILES.planTests, `
use super::*;

fn sha256_hex_is_64_chars() {}
fn require_str_rejects_empty() {}
fn dry_run_plan_sexp_carries_executable_target_hints() {}
fn build_internal_args_for_mission_execution_uses_plan_hints() {}
`);
  writeFixture(root, DEFAULT_FILES.planExecutionRuntime, `
pub(super) async fn action_execute() {}
pub(super) fn action_execute_bridge() {}
pub(super) async fn action_execute_internal() {}
parse_plan_hints(&plan.sexp_text);
let x = (t, "plan_hint");
let target_source = "";
let dispatch_strategy_source = "";
super::super::plan_dag::action_execute_dag_v1;
super::super::workstation_dispatch::run_workstation_dispatch_with_contract_and_trace;
pub(super) async fn compute_workstation_proposal_bundle() {}
pub(super) async fn compute_workstation_auto_spawn_gate() {}
pub(super) fn attach_inference_block() {}
`);
  writeFixture(root, DEFAULT_FILES.planInternalDispatch, `
pub(in crate::handlers::knowledge) fn build_internal_dispatch_args() {
  "mission_execution";
  "mission_task_delegate";
  "mission_flow_run";
  DERIVED_OBJECTIVE_MAX;
  VALID_DELEGATE_INTENTS;
  AGENT_TEAM_OBJECTIVE_HINT;
}
pub(super) fn derive_objective_from_plan() {}
pub(super) fn truncate_chars() {}
pub(in crate::handlers::knowledge) fn tool_result_payload() {}
`);
  writeFixture(root, DEFAULT_FILES.planCompileAuthoring, `
pub(super) async fn action_compile() {}
fn resolve_dry_run_plan_target() { return Ok("mission_task_delegate"); }
fn render_dry_run_plan_sexp() {
  String::from("(plan-draft\\n");
  ":execution-readiness :dry-run-executable-scaffold";
  push_lisp_string_field(&mut out, "target", input.target);
  push_lisp_string_field(&mut out, "objective", input.objective);
  out.push_str("  :nodes\\n");
}
pub(super) fn build_planner_system_prompt() {}
pub(super) fn build_planner_user_prompt() {}
pub(super) fn validate_compiled_plan_sexp() {}
pub(super) async fn maybe_write_plan_artifact() {
  attempt_artifact_write;
}
`);
  writeFixture(root, DEFAULT_FILES.planApprovalReview, `
mod proposer;
mod subscriber;
use self::proposer::{
    attach_plan_apply_gate_block, attach_plan_proposal_block, build_plan_automation_ctx,
    parse_plan_proposer_mode_or_error, plan_proposer_summary, request_plan_auto_approve_proposal,
};
pub(crate) use self::subscriber::{handle_review_resolved_event, PlanSubscriberOutcome};
pub(super) const PLAN_REVIEW_ACTIONS: &[&str] = &["compile", "approve", "mark", "supersede"];
pub(super) async fn action_approve() {}
pub(super) async fn action_mark() {}
pub(super) async fn action_supersede() {}
`);
  writeFixture(root, DEFAULT_FILES.planApprovalProposer, `
use super::*;
pub(super) fn build_plan_automation_ctx() {}
pub(super) async fn request_plan_auto_approve_proposal() {}
fn attach_plan_proposal_block() {}
pub(super) fn attach_plan_apply_gate_block() {}
pub(super) fn parse_plan_proposer_mode_or_error() {}
pub(super) fn plan_proposer_summary() {}
const PLAN_REVIEW_PROPOSER_CALLER: &str = "plan_review_proposer";
const SONNET_PLAN_PROPOSER_MAX_TOKENS: u32 = 1024;
`);
  writeFixture(root, DEFAULT_FILES.planApprovalSubscriber, `
use super::*;
pub(crate) enum PlanSubscriberOutcome {
  MarkNeedsExplicitCall,
  SupersedeNeedsExplicitCall,
}
pub(crate) async fn handle_review_resolved_event() {
  validate_review_resolution_envelope;
  PLAN_REVIEW_ACTIONS;
  PlanStatus::Approved;
  MarkNeedsExplicitCall;
  SupersedeNeedsExplicitCall;
}
`);
  writeFixture(root, DEFAULT_FILES.planFieldInference, `
pub(crate) enum InferPlanFieldsMode {}
pub(crate) fn parse_infer_plan_fields_mode() {}
pub(super) struct PlanFieldInference {}
pub(super) fn compute_plan_field_inference() {}
mod llm;
pub(super) use llm::*;
mod apply;
pub(super) use apply::*;
pub(super) fn apply_safe_augmentation() {}
pub(super) const WORKSTATION_INFER_MODE_SONNET_SUGGEST: &str = "sonnet_suggest";
pub(super) fn parse_workstation_inference_mode() {}
pub(super) fn refuse_workstation_inference_in_dag_mode() {}
const RESPONSE_KEYS: &[&str] = &["plan_field_inference", "persisted_apply"];
`);
  writeFixture(root, DEFAULT_FILES.planFieldInferenceLlm, `
pub(super) const LLM_ALLOWED_FIELDS: &[&str] = &["target"];
pub(super) struct LlmProposal {}
pub(super) struct LlmProposalBundle {}
pub(super) fn parse_llm_proposals() {}
pub(super) fn reconcile_llm_conflicts() {}
pub(super) fn build_llm_inference_prompt() {}
pub(super) async fn request_llm_proposals() {}
pub(super) fn deterministic_covers_all_fields() {}
pub(super) async fn read_recent_evidence_entries() {}
`);
  writeFixture(root, DEFAULT_FILES.planFieldInferenceApply, `
fn apply_safe_augmentation() {}
enum ApplyOrigin {}
struct ApplyGateOutcome {}
fn validate_apply_gate_args() {}
fn compute_apply_gate() {}
enum PersistedApplyStatus {}
struct PersistedApplyOutcome {}
fn enforce_persisted_apply_preflight() {}
async fn execute_persisted_apply() {}
fn attach_persisted_apply_block() {}
`);
  writeFixture(root, DEFAULT_FILES.planExecuteHints, `
pub(crate) struct ParsedPlanHints {}
pub(crate) struct ResolvedExec {}
pub(crate) fn parse_plan_hints() {
  match key.as_str() {
    "target" | "target-tool" | "tool" => {}
    "objective" => store_first(&mut h.objective, &value)
  }
}
pub(crate) fn scan_keyword_pairs() {}
pub(crate) fn normalize_target() {}
pub(crate) fn canonicalize_strategy() {}
pub(crate) fn resolve_dispatch_strategy() {}
const AGENT_TEAM_OBJECTIVE_HINT: &str = "";`);
  writeFixture(root, DEFAULT_FILES.planTaskContract, `
pub(crate) enum TaskContractEmitMode {}
pub(crate) enum DispatchContractMode {}
pub(crate) struct TaskContractInputs {}
pub(crate) fn parse_task_contract_emit_mode() {}
pub(crate) fn parse_dispatch_contract_mode() {}
pub(crate) fn build_task_contract_lisp() {
  ":session-trace-path";
}
pub(crate) fn write_task_contract_under_root() {}
pub(crate) async fn emit_task_contract() {}
pub(crate) fn task_contract_inputs_from_hints() {}
pub(crate) fn task_contract_inputs_from_hints_with_trace() {}`);
  writeFixture(root, DEFAULT_FILES.planDistillChain, `
pub(super) const DISTILL_CHAIN_MODE_RECORD_ONLY: &str = "record_only";
pub(super) const CHAIN_RECORD_KIND: &str = "distill_chain_record";
pub(super) fn parse_distill_chain_id() {}
pub(super) fn parse_distill_chain_name() {}
pub(super) fn parse_distill_chain_mode() {}
pub(super) fn distill_chain_requested() {}
pub(super) fn validate_distill_chain_args() {}
pub(super) fn build_distill_chain_block() {
  "distill_chain_status";
}
pub(super) async fn apply_distill_chain() {
  "mission_workflow";
}
pub(super) fn attach_distill_chain_to_payload() {}`);
  writeFixture(root, DEFAULT_FILES.planDispatchResponse, `
pub(super) fn validate_session_trace_path_arg() {
  "session_trace_path";
}
fn reject_or_warn_trace_path() {}
pub(super) fn attach_session_trace_response_fields() {}
pub(crate) fn merge_task_contract_block() {}
pub(super) fn build_task_contract_failure_response() {
  "task_contract_emit_failed";
}
pub(super) fn build_task_contract_dry_run_response() {}
pub(super) fn build_workstation_dispatch_response() {
  "workstation_dispatch_v0";
}
pub(super) fn build_internal_dispatch_success_response() {
  "status_update_failed";
}`);
  writeFixture(root, DEFAULT_FILES.planEvidenceSidecar, `
pub(super) async fn action_record_evidence() {
  let source_override = "";
  "wrap_legacy_record_evidence";
  "entry_count";
}
pub(crate) async fn append_plan_evidence_entry() {
  "recorded_at";
  "COMPANION_DIR";
}`);
  writeFixture(root, DEFAULT_FILES.planRouterPolicyAdapter, `
mod schema_parser;
pub(super) fn parse_backend_registry(input: &str) { schema_parser::parse_backend_registry(input); }
pub(super) fn parse_router_policy(input: &str) { schema_parser::parse_router_policy(input); }
pub(super) enum RouterPolicyMode { Off, DryRun }
pub(super) fn parse_router_policy_mode() {}
pub(super) fn attach_router_recommendation_block() {}
fn compute_recommendation_block() {
  "router_apply_eligible";
  "router_dispatch_descriptor";
  "backend_readiness_status";
  Value::Bool(false);
}`);
  writeFixture(root, DEFAULT_FILES.planRouterPolicySchemaParser, `
pub(super) fn parse_router_policy() {}
pub(super) fn parse_backend_registry() {}
const READINESS_STATUSES: &[&str] = &[];
fn parse_rule() {}
fn parse_backend_entry() {}
fn parse_clause() {}
enum Sexp {}
enum Token {}
struct TokenCursor;
`);
  writeFixture(root, DEFAULT_FILES.planTaskRunnerAdapter, `
pub(super) enum TaskRunnerMode { Off, DryRun }
pub(super) fn parse_task_runner_mode() {}
pub(super) fn attach_task_runner_block() {}
fn build_runner_response_block() {
  Value::Bool(false);
  "manifest_status";
  "overlap_diagnostics";
  "critical_path_minutes";
  "verification_tier_counts";
}`);
  writeFixture(root, DEFAULT_FILES.planDag, `
fn parse_node_form() {
  match key.as_str() {
    "target" | "target-tool" | "tool" => {}
    "objective" => set_first(&mut objective, &value)
    "timeout-ms" | "timeout_ms" => {}
    "target-project" | "target_project" | "project" => {}
    "requested-cwd" | "requested_cwd" | "cwd" => {}
    "acceptance-commands" | "acceptance_commands" => {}
    "workstation-dispatch" | "workstation_dispatch" => {}
  }
}
node_args.insert("timeout_secs".to_string(), Value::Number(secs.into()));
build_internal_dispatch_args();
run_workstation_dispatch();
mod acceptance;
use acceptance::{
    apply_acceptance_fan_in, derive_acceptance_pause_id, evaluate_node_acceptance,
    AcceptanceEvaluation, AcceptanceMode, AcceptanceRequires, AcceptanceStatus,
};
mod claim_lease;
use claim_lease::{
    build_planned_claims, derive_node_claim_scopes, derive_plan_dag_claim_id, parse_claim_lease_secs,
    parse_claimer_name, parse_enforce_claims, ClaimAcquire, ClaimRegistry, PlanDagClaim,
};
mod rollback;
use rollback::{
    run_cascade_rollback, run_rollback, RollbackCascadeMode, RollbackEvaluation, RollbackPolicy,
    RollbackStatus,
};
mod parser;
pub(super) use parser::{DagNode, ParsedDag};
use parser::{build_validated_dag, ReviewGateKind, FAILURE_POLICY_FAIL_FAST};
mod resume;
pub(super) use resume::action_execute_resume;
#[cfg(test)]
pub(super) use resume::validate_resume_request;
pub(crate) use resume::{handle_review_resolved_plan_node_event, PlanNodeResumeListenerOutcome};
mod projection;
use projection::{build_node_hint_summary, build_nodes_summary, build_retry_plan};
mod scheduler;
use scheduler::{compute_concurrency_plan, parse_max_parallel_nodes, propagate_taint};
mod dispatch;
use dispatch::{dispatch_node, DispatchOutcome, TaskContractDispatchCtx};
mod mode;
pub(super) use mode::{detect_scheduler_mode, refuse_llm_inference_in_dag_mode};
mod finalization;
pub(super) use finalization::parse_finalize_plan;
use finalization::{
    build_distill_block, build_finalization_block, finalize_plan_status_label,
    parse_distill_mode_arg, parse_distill_on_success, validate_finalize_args,
    FINALIZE_DISTILL_MODE_DRY_RUN,
};
mod lifecycle;
use lifecycle::{
    emit_evidence_acceptance, emit_evidence_claim_conflict, emit_evidence_claim_released,
    emit_evidence_claimed, emit_evidence_dag_finalized, emit_evidence_finished,
    emit_evidence_rollback, emit_evidence_running, emit_evidence_skipped,
    emit_paused_review_gate, plan_node_should_retry, publish_plan_node_state_change, EvidenceCtx,
};
#[cfg(test)]
mod tests;`);
  writeFixture(root, DEFAULT_FILES.planDagDispatch, `
pub(super) struct DispatchOutcome;
pub(super) struct TaskContractDispatchCtx;
pub(super) async fn dispatch_node() {
  workstation_dispatch::run_workstation_dispatch_with_contract();
  agent_execution::handle();
  task_delegate::handle();
  flow_run::handle();
  plan::emit_task_contract();
  plan::merge_task_contract_block();
  tool_result_payload();
}
fn node_to_workstation_hints() {}
fn workstation_outcome_to_dispatch_pair() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagParser, `
pub(super) const MAX_NODE_ATTEMPTS_CAP: u32 = 3;
pub(super) const MAX_RETRY_DELAY_MS: u64 = 60000;
pub(in crate::handlers::knowledge) struct DagNode;
impl DagNode {
  pub(super) fn acceptance_mode_kind() {}
  pub(super) fn has_acceptance_fan_in() {}
}
pub(super) enum ReviewGateKind { None }
pub(in crate::handlers::knowledge) struct ParsedDag;
pub(super) enum DagBuildError { NoNodes }
pub(super) fn build_validated_dag() {
  AcceptanceRequires::parse();
  RollbackCascadeMode::parse();
}
pub(super) fn parse_plan_dag() {}
fn scan_top_level_forms() {}
fn parse_node_form() {
  match key.as_str() {
    "target" | "target-tool" | "tool" => {}
    "objective" => set_first(&mut objective, &value)
    "timeout-ms" | "timeout_ms" => {}
    "target-project" | "target_project" | "project" => {}
    "requested-cwd" | "requested_cwd" | "cwd" => {}
    "acceptance-commands" | "acceptance_commands" => {}
    "workstation-dispatch" | "workstation_dispatch" => {}
  }
}
fn scan_keyword_pairs() {}
fn kahn_topo_sort() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagAcceptance, `
pub(super) enum AcceptanceMode { InnerStatus }
pub(super) enum AcceptanceRequires { AllSucceeded }
pub(super) enum AcceptanceStatus { Accepted }
pub(super) struct AcceptanceEvaluation;
pub(super) struct AcceptanceFanInOutcome;
pub(super) fn evaluate_node_acceptance() {}
pub(super) fn apply_acceptance_fan_in() {}
fn inner_payload_failure_signal() {}
fn inner_payload_missing_keys() {}
pub(super) fn derive_acceptance_pause_id() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagClaimLease, `
pub(super) const PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS: i64 = 1800;
pub(super) fn parse_claim_lease_secs() {}
pub(super) fn parse_claimer_name() {}
pub(super) fn parse_enforce_claims() {}
pub(super) fn derive_node_claim_scopes() {}
pub(super) fn derive_plan_dag_claim_id() {}
pub(super) struct PlanDagClaim;
pub(super) enum ClaimAcquire { Acquired }
pub(super) struct ClaimRegistry;
pub(super) fn build_planned_claims() {}
scopes_overlap_pure();
`);
  writeFixture(root, DEFAULT_FILES.planDagRollback, `
impl DagNode {
  pub(super) fn rollback_policy_kind(&self) {}
}
pub(super) enum RollbackPolicy { None }
pub(super) enum RollbackStatus { NotRequested }
pub(super) struct RollbackEvaluation;
pub(super) enum RollbackCascadeMode { None }
pub(super) struct CascadeCompensationOutcome;
pub(super) struct CascadeRollbackOutcome;
pub(super) fn build_rollback_descriptor() {}
pub(super) fn pre_dispatch_rollback_decision() {}
pub(super) async fn run_rollback() {}
pub(super) fn compute_compensation_order() {}
pub(super) fn build_compensation_plan_entry() {}
pub(super) async fn run_cascade_rollback() {}
fn map_dispatch_outcome_to_compensation() {}
fn truncate_rollback_brief_preview() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagResume, `
pub(in crate::handlers::knowledge) enum PlanNodeResumeError { MissingTopicHash }
pub(in crate::handlers::knowledge) fn validate_resume_request() {}
pub(in crate::handlers::knowledge) async fn action_execute_resume() {
  parse_review_question_id_struct();
  PlanNodeResumeInput;
}
fn resume_error_to_tool_result() {}
async fn emit_resume_decision_evidence() {}
pub(crate) enum PlanNodeResumeListenerOutcome { NotFound }
pub(crate) async fn handle_review_resolved_plan_node_event() {
  TaskContractDispatchCtx::off();
}
`);
  writeFixture(root, DEFAULT_FILES.planDagProjection, `
pub(super) fn build_nodes_summary() {
  RollbackPolicy::None;
}
pub(super) fn build_retry_plan() {}
pub(super) fn build_node_hint_summary() {
  unsupported_top_forms;
  unsupported_fields;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagFinalization, `
pub(super) const FINALIZE_DISTILL_MODE_DRY_RUN: &str = "dry_run";
pub(super) const FINALIZE_DISTILL_MODE_SONNET: &str = "sonnet";
pub(in crate::handlers::knowledge) fn parse_finalize_plan() {}
pub(super) fn parse_distill_on_success() {}
pub(super) fn parse_distill_mode_arg() {}
pub(super) fn validate_finalize_args() {}
pub(super) fn finalize_plan_status_label() {}
fn unchanged_status_label() {}
pub(super) fn build_finalization_block() {}
pub(super) fn build_distill_block() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycle, `
pub(super) struct EvidenceCtx;
pub(super) async fn emit_evidence_dag_finalized() { with_state_transition("dag_finalized"); }
pub(super) fn deterministic_plan_node_event_id() {}
pub(super) fn build_plan_node_state_changed_event() {}
pub(super) async fn publish_plan_node_state_change() { EventRef::new(); }
pub(super) fn plan_node_should_retry() {}
pub(super) async fn emit_evidence_running() {}
pub(super) async fn emit_evidence_finished() {}
pub(super) async fn emit_evidence_rollback() {}
pub(super) async fn emit_evidence_acceptance() {}
pub(super) async fn emit_evidence_skipped() {}
pub(super) async fn emit_paused_review_gate() { publish_question(ev); }
pub(super) async fn emit_evidence_claimed() {}
pub(super) async fn emit_evidence_claim_released() {}
pub(super) async fn emit_evidence_claim_conflict() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagScheduler, `
pub(super) fn compute_concurrency_plan() {}
pub(super) fn parse_max_parallel_nodes() {}
pub(super) fn propagate_taint() {}
pub(super) struct NodeInnerArgs;
pub(super) fn build_node_inner_args() {
  node_args.insert("timeout_secs".to_string(), Value::Number(secs.into()));
  build_internal_dispatch_args();
  ParsedPlanHints::default();
}
`);
  writeFixture(root, DEFAULT_FILES.planDagMode, `
pub(in crate::handlers::knowledge) fn detect_scheduler_mode() {
  parse_infer_plan_fields_mode();
}
pub(in crate::handlers::knowledge) fn refuse_llm_inference_in_dag_mode() {
  parse_infer_plan_fields_mode();
  is_llm_augmented();
  "infer_plan_fields=\`{}\` is single-node-execute-only in v0";
}
`);
  writeFixture(root, DEFAULT_FILES.planDagTests, `
use super::*;
use super::acceptance::*;
use super::claim_lease::*;
use super::finalization::*;
use super::lifecycle::*;
use super::mode::*;
use super::parser::*;
use super::projection::*;
use super::resume::*;
use super::rollback::*;
use super::scheduler::*;
fn parse_plan_dag_extracts_explicit_node_forms() {}
fn build_validated_dag_accepts_valid_chain() {}
fn validate_resume_request_routes_unique_paused_node() {}
fn claim_registry_rejects_overlapping_scope() {}
fn task_contract_dispatch_ctx_captures_machine_mode_for_dag() {}
`);
  writeFixture(root, DEFAULT_FILES.unifiedEntryPlanner, `
pub(crate) fn plan_pipeline() {}
PipelineDecision::PlanCompile;
PipelineDecision::PlanExecute;
build_plan_compile_args();
build_plan_execute_args();
"compiler_mode" "target" "dispatch_strategy" "target_project" "objective" "requested_cwd" "flow_id" "timeout_secs" "infer_plan_fields";
let approved_plan_id = "";
let execute_flag = true;`);
  writeFixture(root, DEFAULT_FILES.mcpPlan, `
[compile dry_run | execute] compile dry_run renders this into PLAN.lisp as :target
runner scans plan.sexp_text for :target / :target-tool / :tool hints
Source-resolution precedence is explicit_arg > plan_hint > missing
[execute internal mission_task_delegate] override the auto-derived objective
[execute internal mission_task_delegate] passthrough timeout
supported per-node fields: id / target / objective / depends-on / condition / failure-policy / timeout-ms / dispatch-strategy / target-project / requested-cwd / flow-id
Declaring \`:acceptance-commands\` without a typed \`:acceptance-mode\` defaults to \`manual_required\``);
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
