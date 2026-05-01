#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

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
  planCompileArtifact:
    'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring/artifact.rs',
  planCompileValidation:
    'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring/validation.rs',
  planApprovalReview: 'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs',
  planApprovalApprove: 'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/approve.rs',
  planApprovalMark: 'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/mark.rs',
  planApprovalProposer:
    'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs',
  planApprovalSubscriber:
    'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs',
  planApprovalSupersede:
    'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/supersede.rs',
  planFieldInference: 'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference.rs',
  planFieldInferenceMode:
    'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/mode.rs',
  planFieldInferenceEvidence:
    'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/evidence.rs',
  planFieldInferenceRules:
    'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/rules.rs',
  planFieldInferenceLlm: 'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/llm.rs',
  planFieldInferenceApply: 'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply.rs',
  planFieldInferenceApplyPersisted:
    'crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply/persisted.rs',
  planExecutionRuntime: 'crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs',
  planExecutionBridge:
    'crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/bridge.rs',
  planExecutionInternal:
    'crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/internal.rs',
  planExecutionWorkstation:
    'crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/workstation.rs',
  planInternalDispatch: 'crates/missiond-daemon/src/handlers/knowledge/plan/internal_dispatch.rs',
  planExecuteHints: 'crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs',
  planTaskContract: 'crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs',
  planDistillChain: 'crates/missiond-daemon/src/handlers/knowledge/plan/distill_chain.rs',
  planDispatchResponse: 'crates/missiond-daemon/src/handlers/knowledge/plan/dispatch_response.rs',
  planEvidenceSidecar: 'crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs',
  planRouterPolicyAdapter: 'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs',
  planRouterPolicyPredicate:
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs',
  planRouterPolicyReadiness:
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs',
  planRouterPolicyDescriptor:
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs',
  planRouterPolicySchemaParser:
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs',
  planTaskRunnerAdapter: 'crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs',
  planTaskRunnerManifest:
    'crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/manifest.rs',
  planTaskRunnerProjection:
    'crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/projection.rs',
  planDag: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs',
  planDagRuntime: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime.rs',
  planDagRuntimeAcceptance:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/acceptance.rs',
  planDagRuntimeBookkeeping:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/bookkeeping.rs',
  planDagRuntimeClaiming:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/claiming.rs',
  planDagRuntimeClaims:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/claims.rs',
  planDagRuntimeDrain:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/drain.rs',
  planDagRuntimeFailures:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/failures.rs',
  planDagRuntimeGates:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/gates.rs',
  planDagRuntimeRollbacks:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/rollbacks.rs',
  planDagRuntimeRetry:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/retry.rs',
  planDagRuntimeSkips:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/skips.rs',
  planDagRuntimeSpawn:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/spawn.rs',
  planDagRuntimeSuccess:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/success.rs',
  planDagParser: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser.rs',
  planDagParserTypes: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types.rs',
  planDagParserTypesNode:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types/node.rs',
  planDagParserTypesErrors:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types/errors.rs',
  planDagParserScanner: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner.rs',
  planDagParserScannerTopLevel:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/top_level.rs',
  planDagParserScannerNodeForm:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/node_form.rs',
  planDagParserScannerLists:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/lists.rs',
  planDagParserScannerKeywordPairs:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/keyword_pairs.rs',
  planDagParserValidation:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/validation.rs',
  planDagAcceptance: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance.rs',
  planDagAcceptanceTypes:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/types.rs',
  planDagAcceptanceEvaluator:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/evaluator.rs',
  planDagAcceptanceFanIn:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/fan_in.rs',
  planDagAcceptancePayload:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/payload.rs',
  planDagAcceptancePause:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/pause.rs',
  planDagClaimLease: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs',
  planDagDispatch: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch.rs',
  planDagDispatchTypes:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/types.rs',
  planDagDispatchWorkstation:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/workstation.rs',
  planDagDispatchTaskContractCtx:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/task_contract_ctx.rs',
  planDagDispatchRunner:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/runner.rs',
  planDagRollback: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback.rs',
  planDagRollbackDescriptor:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/descriptor.rs',
  planDagRollbackRun: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/run.rs',
  planDagRollbackTypes:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types.rs',
  planDagRollbackTypesNodeExt:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/node_ext.rs',
  planDagRollbackTypesPolicy:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/policy.rs',
  planDagRollbackTypesEvaluation:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/evaluation.rs',
  planDagRollbackTypesCascade:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/cascade.rs',
  planDagRollbackCascade:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade.rs',
  planDagRollbackCascadeOrdering:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/ordering.rs',
  planDagRollbackCascadePlanEntry:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/plan_entry.rs',
  planDagRollbackCascadeRunner:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/runner.rs',
  planDagRollbackCascadeDispatchOutcome:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/dispatch_outcome.rs',
  planDagResume: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume.rs',
  planDagResumeAction:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/action.rs',
  planDagResumeEvidence:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/evidence.rs',
  planDagResumeListener:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/listener.rs',
  planDagResumeValidation:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/validation.rs',
  planDagOutcome: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome.rs',
  planDagOutcomeState:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/state.rs',
  planDagOutcomeNodeResult:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/node_result.rs',
  planDagOutcomeExecution:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/execution.rs',
  planDagProjection: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/projection.rs',
  planDagFinalization: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/finalization.rs',
  planDagLifecycle: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle.rs',
  planDagLifecycleContext:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/context.rs',
  planDagLifecycleEventRef:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/event_ref.rs',
  planDagLifecycleFinalize:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/finalize.rs',
  planDagLifecycleNodes:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes.rs',
  planDagLifecycleNodesRunning:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/running.rs',
  planDagLifecycleNodesFinished:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/finished.rs',
  planDagLifecycleNodesRollback:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/rollback.rs',
  planDagLifecycleNodesAcceptance:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/acceptance.rs',
  planDagLifecycleNodesSkipped:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/skipped.rs',
  planDagLifecycleRetry:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/retry.rs',
  planDagLifecycleReview:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/review.rs',
  planDagLifecycleClaims:
    'crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/claims.rs',
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
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
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
    'plan/compile_authoring/artifact.rs owns plan file-first artifact egress',
    'plan/compile_authoring/validation.rs owns planner prompt and Lisp output validation',
    'plan/approval_review.rs owns mission_plan plan-review-gate caller action facade',
    'plan/approval_review/approve.rs owns mission_plan approve action',
    'plan/approval_review/mark.rs owns mission_plan mark action',
    'plan/approval_review/proposer.rs owns mission_plan plan-review LLM proposal helpers',
    'plan/approval_review/subscriber.rs owns mission_plan plan-review subscriber bridge',
    'plan/approval_review/supersede.rs owns mission_plan supersede action',
    'plan/field_inference.rs owns mission_plan execute preflight field inference/core',
    'plan/field_inference/mode.rs owns infer_plan_fields/workstation_inference_mode parsing',
    'plan/field_inference/evidence.rs owns evidence-sidecar scanners',
    'plan/field_inference/rules.rs owns deterministic field inference rules',
    'plan/field_inference/llm.rs owns Sonnet proposal parsing',
    'plan/field_inference/apply.rs owns apply_gate',
    'plan/field_inference/apply/persisted.rs owns persisted_apply',
    'plan/execution_runtime.rs owns mission_plan execute facade orchestration',
    'plan/execution_runtime/bridge.rs owns bridge descriptor projection',
    'plan/execution_runtime/internal.rs owns mission_plan internal dispatch runtime',
    'PLAN_RUNNER_EVENT_REF_UNAVAILABLE_REASON',
    'plan/execution_runtime/workstation.rs owns workstation proposal/auto-spawn execution adjuncts',
    'plan/internal_dispatch.rs owns mission_plan inner target argument projection',
    'plan/execute_hints.rs owns mission_plan PLAN.lisp hint parsing',
    'plan/task_contract.rs owns mission_plan task-contract Lisp projection',
    'plan/distill_chain.rs owns mission_plan cross-plan distill-chain egress',
    'plan/dispatch_response.rs owns mission_plan execution response egress',
    'plan/evidence_sidecar.rs owns mission_plan evidence sidecar egress',
    'plan/router_policy_dry_run.rs owns the mission_plan router-policy adapter',
    'plan/router_policy_dry_run/predicate.rs owns router-policy predicate projection',
    'plan/router_policy_dry_run/readiness.rs owns router-policy trace-index/backend-readiness projection',
    'plan/router_policy_dry_run/descriptor.rs owns router dispatch descriptor projection',
    'plan/router_policy_dry_run/schema_parser.rs owns the router-policy Lisp schema parser',
    'plan/task_runner_dry_run.rs owns the mission_plan task-runner adapter',
    'plan/task_runner_dry_run/manifest.rs owns task-runner manifest loading/parsing',
    'plan/task_runner_dry_run/projection.rs owns task-runner manifest response projection',
    'plan/tests.rs holds the historical mission_plan regression suite outside the runtime facade',
    'plan_dag/parser.rs is the DAG parser/validator facade',
    'plan_dag/parser/types.rs is the DAG parser types facade',
    'plan_dag/parser/types/node.rs owns DAG node shapes and typed hint projections',
    'plan_dag/parser/types/errors.rs owns DAG build error egress',
    'plan_dag/parser/scanner.rs is the DAG scanner facade',
    'plan_dag/parser/scanner/top_level.rs owns top-level PLAN.lisp S-expression scanning',
    'plan_dag/parser/scanner/node_form.rs owns node form keyword lowering',
    'plan_dag/parser/scanner/lists.rs owns DAG id-list parsing',
    'plan_dag/parser/scanner/keyword_pairs.rs owns Lisp keyword/value token scanning',
    'plan_dag/parser/validation.rs owns DAG contract validation/topological ordering',
    'plan_dag/acceptance.rs is the DAG acceptance facade',
    'plan_dag/acceptance/types.rs owns typed acceptance contracts',
    'plan_dag/acceptance/evaluator.rs owns per-node acceptance evaluation',
    'plan_dag/acceptance/fan_in.rs owns cross-node acceptance fan-in',
    'plan_dag/acceptance/payload.rs owns inner-payload signal/key scanning',
    'plan_dag/acceptance/pause.rs owns deterministic acceptance pause ids',
    'plan_dag/runtime.rs owns the DAG live runtime wave loop',
    'plan_dag/runtime/acceptance.rs owns DAG runtime success acceptance projection',
    'plan_dag/runtime/bookkeeping.rs owns DAG runtime bookkeeping',
    'plan_dag/runtime/claiming.rs owns DAG runtime dispatch claim preparation',
    'plan_dag/runtime/claims.rs owns DAG runtime claim acquisition and release projection',
    'plan_dag/runtime/drain.rs owns DAG runtime wave drain projection',
    'plan_dag/runtime/failures.rs owns DAG runtime final failure projection',
    'plan_dag/runtime/gates.rs owns DAG runtime ready-node gate filtering',
    'plan_dag/runtime/rollbacks.rs owns DAG runtime rollback evaluation',
    'plan_dag/runtime/retry.rs owns DAG runtime retry projection',
    'plan_dag/runtime/skips.rs owns DAG runtime skip materialization',
    'plan_dag/runtime/spawn.rs owns DAG runtime dispatch spawn projection',
    'plan_dag/runtime/success.rs owns DAG runtime successful dispatch projection',
    'plan_dag/dispatch.rs is the DAG node dispatch facade',
    'plan_dag/dispatch/types.rs owns DAG dispatch outcome shape',
    'plan_dag/dispatch/workstation.rs owns workstation hint/outcome projection',
    'plan_dag/dispatch/task_contract_ctx.rs owns per-run task-contract dispatch context',
    'plan_dag/dispatch/runner.rs owns DAG node dispatch execution',
    'plan_dag/tests.rs does the same for the DAG scheduler regression suite',
    'plan_dag/rollback.rs is the DAG rollback facade',
    'plan_dag/rollback/types.rs is the rollback types facade',
    'plan_dag/rollback/types/node_ext.rs owns DagNode rollback hint projections',
    'plan_dag/rollback/types/policy.rs owns rollback policy parsing',
    'plan_dag/rollback/types/evaluation.rs owns node-local rollback evaluation JSON',
    'plan_dag/rollback/types/cascade.rs owns cascade rollback outcome JSON',
    'plan_dag/rollback/descriptor.rs owns rollback descriptor and pre-dispatch safety',
    'plan_dag/rollback/run.rs owns node-local rollback execution',
    'plan_dag/rollback/cascade.rs is the DAG cascade rollback facade',
    'plan_dag/rollback/cascade/ordering.rs owns compensation ordering',
    'plan_dag/rollback/cascade/plan_entry.rs owns plan-mode compensation projection',
    'plan_dag/rollback/cascade/runner.rs owns dispatch-safe cascade execution',
    'plan_dag/rollback/cascade/dispatch_outcome.rs owns workstation-dispatch outcome mapping',
    'plan_dag/resume.rs is the DAG review-resume facade',
    'plan_dag/resume/validation.rs owns resume id validation and error vocabulary',
    'plan_dag/resume/action.rs owns explicit resume action entry/egress',
    'plan_dag/resume/evidence.rs owns resume decision evidence',
    'plan_dag/resume/listener.rs owns bus-resolved plan-node resume bridge',
    'plan_dag/outcome.rs is the DAG node outcome facade',
    'plan_dag/outcome/state.rs owns node state/lifecycle enums',
    'plan_dag/outcome/node_result.rs owns node result shape/defaults',
    'plan_dag/outcome/execution.rs owns DAG execution outcome response projection',
    'plan_dag/projection.rs owns the DAG response projection core',
    'plan_dag/finalization.rs owns the DAG finalization projection core',
    'plan_dag/lifecycle.rs is the DAG lifecycle facade',
    'plan_dag/lifecycle/context.rs owns per-run evidence context',
    'plan_dag/lifecycle/event_ref.rs owns deterministic event refs and bus publish fallback',
    'plan_dag/lifecycle/finalize.rs owns dag_finalized evidence rows',
    'plan_dag/lifecycle/nodes.rs is the DAG node evidence row facade',
    'plan_dag/lifecycle/nodes/running.rs owns ready->running evidence rows',
    'plan_dag/lifecycle/nodes/finished.rs owns running->finished evidence rows',
    'plan_dag/lifecycle/nodes/rollback.rs owns failed->rollback evidence rows',
    'plan_dag/lifecycle/nodes/acceptance.rs owns succeeded->acceptance evidence rows',
    'plan_dag/lifecycle/nodes/skipped.rs owns pending->skipped evidence rows',
    'plan_dag/lifecycle/retry.rs owns retry attempt constants and retry predicate',
    'plan_dag/lifecycle/review.rs owns paused review-gate evidence rows',
    'plan_dag/lifecycle/claims.rs owns the DAG claim lifecycle evidence rows',
    'plan_dag/scheduler.rs owns the DAG scheduler projection core',
    'plan_dag/mode.rs owns the DAG scheduler-mode gate',
    'execute can derive target_source=plan_hint from plan.sexp_text',
    'DAG execution parses node-local Lisp hints',
    'node scripts/check-v3-plan-execution-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.planHandler, sources.planHandler, [
    'RouterRuntimeConfig',
    'pub(super) fn load_sonnet_compiler_model',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'queued_sonnet_model',
    'mod compile_authoring',
    'use compile_authoring::{action_compile, collect_string_list}',
    'mod execution_runtime',
    'use execution_runtime::action_execute',
    'mod internal_dispatch',
    'pub(super) use internal_dispatch::{build_internal_dispatch_args, tool_result_payload}',
    '#[cfg(test)]',
    'mod tests;',
  ]);
  forbidAll(diagnostics, files.planHandler, sources.planHandler, [
    'const SONNET_COMPILER_MODEL',
    'SONNET_COMPILER_MODEL: &str = "claude-sonnet"',
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
    'parse_plan_hints(&plan.sexp_text)',
    '(t, "plan_hint")',
    'target_source',
    'dispatch_strategy_source',
    'super::super::plan_dag::action_execute_dag_v1',
    'mod bridge',
    'pub(super) use bridge::{action_execute_bridge, attach_inference_block}',
    'mod internal',
    'pub(super) use internal::action_execute_internal',
    'mod workstation',
    'pub(super) use workstation::{',
  ]);

  requireAll(diagnostics, files.planExecutionBridge, sources.planExecutionBridge, [
    'pub(in crate::handlers::knowledge::plan) fn attach_inference_block',
    'pub(in crate::handlers::knowledge::plan) fn action_execute_bridge',
    '"bridge_ready"',
    '"runner_status": "bridge_only"',
    'next_call',
  ]);

  requireAll(diagnostics, files.planExecutionInternal, sources.planExecutionInternal, [
    'pub(in crate::handlers::knowledge::plan) async fn action_execute_internal',
    'PLAN_RUNNER_EVENT_REF_UNAVAILABLE_REASON',
    'parse_task_contract_emit_mode',
    'validate_session_trace_path_arg',
    'workstation_dispatch::run_workstation_dispatch_with_contract_and_trace',
    'build_internal_dispatch_args',
    'agent_execution::handle',
    'task_delegate::handle',
    'flow_run::handle',
    'evidence_collector::append',
    'build_internal_dispatch_success_response',
  ]);

  requireAll(diagnostics, files.planExecutionWorkstation, sources.planExecutionWorkstation, [
    'pub(in crate::handlers::knowledge::plan) async fn compute_workstation_proposal_bundle',
    'pub(in crate::handlers::knowledge::plan) fn plan_hints_carry_workstation_signal',
    'pub(in crate::handlers::knowledge::plan) fn attach_workstation_proposals_block',
    'pub(in crate::handlers::knowledge::plan) async fn compute_workstation_auto_spawn_gate',
    'pub(in crate::handlers::knowledge::plan) fn attach_workstation_auto_spawn_gate_block',
    'WorkstationProposalGate',
    'request_workstation_proposals',
    'evaluate_workstation_auto_spawn_gate',
    'run_workstation_dispatch_with_contract',
    'workstation_auto_spawn_gate',
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
    'mod artifact',
    'mod validation',
    'pub(super) use artifact::{extract_plan_file_args, maybe_write_plan_artifact}',
    'pub(super) use validation::{',
    'pub(super) fn resolve_dry_run_plan_target',
    'return Ok("mission_task_delegate");',
    'pub(super) fn render_dry_run_plan_sexp',
    'load_sonnet_compiler_model()',
    'compiler_model.clone()',
    'String::from("(plan-draft\\n")',
    ':execution-readiness :dry-run-executable-scaffold',
    'push_lisp_string_field(&mut out, "target", input.target);',
    'push_lisp_string_field(&mut out, "objective", input.objective);',
    'out.push_str("  :nodes\\n");',
  ]);
  forbidAll(diagnostics, files.planCompileAuthoring, sources.planCompileAuthoring, [
    'SONNET_COMPILER_MODEL',
  ]);

  requireAll(diagnostics, files.planCompileArtifact, sources.planCompileArtifact, [
    'pub(in crate::handlers::knowledge::plan) struct PlanFileArgs',
    'pub(in crate::handlers::knowledge::plan) fn extract_plan_file_args',
    'pub(in crate::handlers::knowledge::plan) async fn maybe_write_plan_artifact',
    'attempt_artifact_write',
    'ArtifactKind::Plan',
    'outcome.splice_into(payload)',
  ]);

  requireAll(diagnostics, files.planCompileValidation, sources.planCompileValidation, [
    'pub(in crate::handlers::knowledge::plan) fn collect_string_list',
    'pub(in crate::handlers::knowledge::plan) fn build_planner_system_prompt',
    'pub(in crate::handlers::knowledge::plan) fn build_planner_user_prompt',
    'pub(in crate::handlers::knowledge::plan) struct SexpValidationError',
    'pub(in crate::handlers::knowledge::plan) fn validate_compiled_plan_sexp',
    'pub(in crate::handlers::knowledge::plan) fn strip_fenced_code_block',
    'pub(in crate::handlers::knowledge::plan) fn parens_balanced',
    'pub(in crate::handlers::knowledge::plan) fn top_level_head',
    'ALLOWED_PLAN_HEADS',
    'board_task_id',
  ]);

  requireAll(diagnostics, files.planApprovalReview, sources.planApprovalReview, [
    'mod approve;',
    'mod mark;',
    'mod proposer;',
    'mod subscriber;',
    'mod supersede;',
    'pub(super) use self::approve::action_approve;',
    'pub(super) use self::mark::action_mark;',
    'use self::proposer::{',
    'request_plan_auto_approve_proposal',
    'pub(crate) use self::subscriber::{handle_review_resolved_event, PlanSubscriberOutcome};',
    'pub(super) use self::supersede::action_supersede;',
    'pub(super) const PLAN_REVIEW_ACTIONS',
  ]);

  requireAll(diagnostics, files.planApprovalApprove, sources.planApprovalApprove, [
    'use super::*;',
    'async fn action_approve',
    'async fn action_approve_with_resolution',
    'async fn plan_action_approve_with_policy_only',
    'PlanStatus::Approved',
    'parse_review_resolution_input(args)',
    'maybe_emit_review_question_resolved',
    'attach_plan_apply_gate_block',
  ]);

  requireAll(diagnostics, files.planApprovalMark, sources.planApprovalMark, [
    'use super::*;',
    'async fn action_mark',
    'async fn action_mark_with_resolution',
    'async fn plan_action_mark_with_policy_only',
    'PlanStatus::Approved',
    'parse_review_resolution_input(args)',
    'maybe_emit_review_question_resolved',
    'target_raw',
  ]);

  requireAll(diagnostics, files.planApprovalSupersede, sources.planApprovalSupersede, [
    'use super::*;',
    'async fn action_supersede',
    'async fn action_supersede_with_resolution',
    'async fn plan_action_supersede_with_policy_only',
    'PlanStatus::Superseded',
    'parse_review_resolution_input(args)',
    'maybe_emit_review_question_resolved',
    'destructive',
  ]);

  requireAll(diagnostics, files.planApprovalProposer, sources.planApprovalProposer, [
    'use super::*;',
    'pub(super) fn build_plan_automation_ctx',
    'pub(super) async fn request_plan_auto_approve_proposal',
    'fn attach_plan_proposal_block',
    'pub(super) fn attach_plan_apply_gate_block',
    'pub(super) fn parse_plan_proposer_mode_or_error',
    'pub(super) fn plan_proposer_summary',
    'load_sonnet_compiler_model()',
    'PLAN_REVIEW_PROPOSER_CALLER',
    'SONNET_PLAN_PROPOSER_MAX_TOKENS',
  ]);
  forbidAll(diagnostics, files.planApprovalProposer, sources.planApprovalProposer, [
    'SONNET_COMPILER_MODEL',
    'Some("claude-sonnet".to_string())',
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
    'mod mode;',
    'pub(crate) use mode::{parse_infer_plan_fields_mode, InferPlanFieldsMode};',
    'pub(super) use mode::{',
    'mod evidence;',
    'use evidence::*;',
    'mod rules;',
    'pub(super) use rules::{',
    'pub(super) struct PlanFieldInference',
    'mod llm;',
    'pub(super) use llm::*;',
    'mod apply;',
    'pub(super) use apply::*;',
    'plan_field_inference',
    'persisted_apply',
  ]);

  requireAll(diagnostics, files.planFieldInferenceMode, sources.planFieldInferenceMode, [
    'pub(crate) enum InferPlanFieldsMode',
    'pub(crate) fn parse_infer_plan_fields_mode',
    'pub(in crate::handlers::knowledge::plan) const WORKSTATION_INFER_MODE_SONNET_SUGGEST',
    'pub(in crate::handlers::knowledge::plan) enum WorkstationInferenceMode',
    'pub(in crate::handlers::knowledge::plan) fn parse_workstation_inference_mode',
    'pub(in crate::handlers::knowledge::plan) fn refuse_workstation_inference_in_dag_mode',
    'scheduler_mode',
    'dag_v1',
  ]);

  requireAll(diagnostics, files.planFieldInferenceEvidence, sources.planFieldInferenceEvidence, [
    'pub(super) fn scan_evidence_string_field',
    'pub(super) fn scan_evidence_string_counts',
    'pub(super) fn scan_evidence_string_list',
    'fn pluck_string',
    'fn pluck_string_list',
    'typed_evidence',
  ]);

  requireAll(diagnostics, files.planFieldInferenceRules, sources.planFieldInferenceRules, [
    'pub(in crate::handlers::knowledge::plan) struct PlanInferenceInput',
    'pub(in crate::handlers::knowledge::plan) fn compute_plan_field_inference',
    'pub(in crate::handlers::knowledge::plan) fn caller_str',
    'pub(in crate::handlers::knowledge::plan) fn caller_bool',
    'pub(in crate::handlers::knowledge::plan) fn caller_string_list',
    'pub(super) fn infer_target',
    'pub(super) fn infer_dispatch_strategy',
    'pub(super) fn infer_workstation_dispatch',
    'pub(super) fn finalize_string_field',
    'pub(super) fn finalize_bool_field',
  ]);

  requireAll(diagnostics, files.planFieldInferenceLlm, sources.planFieldInferenceLlm, [
    'const LLM_ALLOWED_FIELDS',
    'struct LlmProposal',
    'struct LlmProposalBundle',
    'fn parse_llm_proposals',
    'fn reconcile_llm_conflicts',
    'fn build_llm_inference_prompt',
    'async fn request_llm_proposals',
    'load_sonnet_compiler_model()',
    'fn deterministic_covers_all_fields',
    'async fn read_recent_evidence_entries',
  ]);
  forbidAll(diagnostics, files.planFieldInferenceLlm, sources.planFieldInferenceLlm, [
    'SONNET_COMPILER_MODEL',
    'Some("claude-sonnet".to_string())',
  ]);

  requireAll(diagnostics, files.planFieldInferenceApply, sources.planFieldInferenceApply, [
    'fn apply_safe_augmentation',
    'enum ApplyOrigin',
    'struct ApplyGateOutcome',
    'fn validate_apply_gate_args',
    'fn compute_apply_gate',
    'mod persisted;',
    'pub(in crate::handlers::knowledge::plan) use persisted::*;',
  ]);

  requireAll(diagnostics, files.planFieldInferenceApplyPersisted, sources.planFieldInferenceApplyPersisted, [
    'use super::*;',
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
    'existing_plan_evidence_sidecar_path',
    'source_override',
  ]);

  requireAll(diagnostics, files.planRouterPolicyAdapter, sources.planRouterPolicyAdapter, [
    'mod descriptor',
    'mod predicate',
    'mod readiness',
    'mod schema_parser',
    'use descriptor::attach_router_dispatch_descriptor',
    'use predicate::{arg_string, evaluate_clause, project_context}',
    'schema_parser::parse_backend_registry(input)',
    'schema_parser::parse_router_policy(input)',
    'pub(super) enum RouterPolicyMode',
    'pub(super) fn parse_router_policy_mode',
    'pub(super) fn attach_router_recommendation_block',
    'fn compute_recommendation_block',
    'load_trace_index',
    'load_backend_registry',
  ]);

  requireAll(diagnostics, files.planRouterPolicyPredicate, sources.planRouterPolicyPredicate, [
    'pub(super) struct PredicateContext',
    'pub(super) fn project_context',
    'pub(super) fn arg_string',
    'pub(super) fn evaluate_clause',
    'struct GlobRegex',
    'fn glob_match',
    'owned_files',
    'path-glob no match',
  ]);

  requireAll(diagnostics, files.planRouterPolicyReadiness, sources.planRouterPolicyReadiness, [
    'pub(super) enum TraceIndexInfo',
    'pub(super) enum BackendRegistryInfo',
    'pub(super) struct BackendEntry',
    'pub(super) fn load_trace_index',
    'pub(super) fn load_backend_registry',
    'pub(super) fn attach_backend_readiness_fields',
    'pub(super) fn attach_trace_index_fields',
    'pub(super) fn computed_block',
    'RICH_TRACE_THRESHOLD',
    'router_apply_eligible',
    'backend_readiness_status',
  ]);

  requireAll(diagnostics, files.planRouterPolicyDescriptor, sources.planRouterPolicyDescriptor, [
    'pub(super) fn attach_router_dispatch_descriptor',
    'missiond.router-dispatch-descriptor.v1',
    'descriptor_status',
    'registry_missing',
    'source_backend_registry_path',
    'router_dispatch_descriptor',
    'Value::Bool(true)',
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
    'mod manifest',
    'mod projection',
    'use manifest::load_runner_inputs',
    'use projection::build_runner_response_block',
    'pub(super) enum TaskRunnerMode',
    'pub(super) fn parse_task_runner_mode',
    'pub(super) fn attach_task_runner_block',
    'pub(super) fn compute_runner_block',
  ]);

  requireAll(diagnostics, files.planTaskRunnerManifest, sources.planTaskRunnerManifest, [
    'pub(super) fn load_runner_inputs',
    'pub(super) enum ManifestStatus',
    'pub(super) struct RunnerInputs',
    'pub(super) struct Manifest',
    'pub(super) struct ManifestNode',
    'fn parse_manifest',
    'fn parse_node_entry',
    'enum Sexp',
    'enum Token',
    'struct TokenCursor',
    'MANIFEST_SCHEMA',
  ]);

  requireAll(diagnostics, files.planTaskRunnerProjection, sources.planTaskRunnerProjection, [
    'pub(super) fn build_runner_response_block',
    'fn project_manifest',
    'struct ManifestProjection',
    'struct VerificationTierCounts',
    'fn collect_overlap_diagnostics',
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
    'mod outcome;',
    'use outcome::{ExecutionOutcome, NodeResult, NodeState};',
    'mod parser;',
    'pub(super) use parser::{DagNode, ParsedDag};',
    'use parser::{build_validated_dag, ReviewGateKind};',
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
    'mod runtime;',
    'use runtime::execute_with_concurrency;',
    '#[cfg(test)]',
    'mod tests;',
  ]);

  requireAll(diagnostics, files.planDagRuntime, sources.planDagRuntime, [
    'mod acceptance;',
    'mod bookkeeping;',
    'mod claiming;',
    'mod claims;',
    'mod drain;',
    'mod failures;',
    'mod gates;',
    'mod rollbacks;',
    'mod retry;',
    'mod skips;',
    'mod spawn;',
    'mod success;',
    'build_node_map(&parsed.nodes)',
    'compute_ready_ids(order, &lifecycle, &by_id)',
    'materialize_tainted_pending_skips(',
    'force_skip_fail_fast_pending(',
    'filter_ready_nodes_for_gates(',
    'pub(super) async fn execute_with_concurrency',
    'tokio::task::JoinSet<Result<DispatchOutcome>>',
    'ClaimRegistry::new()',
    'prepare_dispatch_claim(',
    'spawn_dispatch_attempt(',
    'drain_dispatch_wave(',
    'outcome.results = stitch_results_topologically(results_by_id, &topo_index);',
  ]);

  requireAll(diagnostics, files.planDagRuntimeAcceptance, sources.planDagRuntimeAcceptance, [
    'pub(super) struct SuccessAcceptanceOutcome',
    'pub(super) async fn evaluate_success_acceptance',
    'evaluate_node_acceptance(node, inner_payload, true)',
    'apply_acceptance_fan_in',
    'emit_evidence_acceptance',
    'derive_acceptance_pause_id',
    'AcceptanceStatus::Rejected',
    'NodeState::Paused',
    'NodeLifecycle::Paused',
  ]);

  requireAll(diagnostics, files.planDagRuntimeBookkeeping, sources.planDagRuntimeBookkeeping, [
    'pub(super) fn build_node_map',
    'pub(super) fn build_successor_map',
    'pub(super) fn compute_ready_ids',
    'pub(super) fn stitch_results_topologically',
    'NodeState::SkippedUpstreamFailed',
    'NodeLifecycle::Succeeded',
  ]);

  requireAll(diagnostics, files.planDagRuntimeClaiming, sources.planDagRuntimeClaiming, [
    'pub(super) enum DispatchClaimDecision',
    'pub(super) async fn prepare_dispatch_claim',
    'derive_node_claim_scopes',
    'derive_plan_dag_claim_id',
    'ClaimAcquire::Acquired',
    'record_acquired_claim',
    'ClaimAcquire::Conflict',
    'emit_evidence_claim_conflict',
    'NodeState::Failed',
    'record_compat_claim',
    'propagate_taint',
    'FAILURE_POLICY_FAIL_FAST',
  ]);

  requireAll(diagnostics, files.planDagRuntimeClaims, sources.planDagRuntimeClaims, [
    'pub(super) async fn record_acquired_claim',
    'pub(super) async fn record_compat_claim',
    'emit_evidence_claimed',
    'NodeLifecycle::Claimed',
    'active_claims_by_node.insert(node.id.clone(), claim.claim_id.clone())',
    'pub(super) async fn release_claim_if_recorded',
    'active_claims_by_node.remove(&node.id)',
    'claim_registry.release(&claim_id, chrono::Utc::now())',
    'emit_evidence_claim_released',
  ]);

  requireAll(diagnostics, files.planDagRuntimeDrain, sources.planDagRuntimeDrain, [
    'pub(super) async fn drain_dispatch_wave',
    'join_set.join_next().await',
    'emit_evidence_finished',
    'record_successful_dispatch',
    'retry_failed_node_if_allowed',
    'record_final_failure',
    'DispatchOutcome',
    'classification.is_ok()',
    'abort_aborter = Some(node_id.clone())',
    'DAG scheduler: dispatch task join failed',
  ]);

  requireAll(diagnostics, files.planDagRuntimeFailures, sources.planDagRuntimeFailures, [
    'pub(super) async fn record_final_failure',
    'NodeLifecycle::Failed',
    'release_claim_if_recorded',
    'NodeState::Failed',
    'evaluate_and_emit_rollback',
    'retry_skipped_non_retryable: non_retryable',
    'propagate_taint',
    'FAILURE_POLICY_FAIL_FAST',
  ]);

  requireAll(diagnostics, files.planDagRuntimeGates, sources.planDagRuntimeGates, [
    'pub(super) async fn filter_ready_nodes_for_gates',
    'ReviewGateKind::QuestionEvent',
    'emit_paused_review_gate',
    'emit_evidence_skipped',
    'NodeState::SkippedCondition',
    'NodeState::Paused',
    'propagate_taint',
  ]);

  requireAll(diagnostics, files.planDagRuntimeRollbacks, sources.planDagRuntimeRollbacks, [
    'pub(super) async fn evaluate_and_emit_rollback',
    'run_rollback',
    'run_cascade_rollback',
    'emit_evidence_rollback',
    'node.has_active_rollback_cascade()',
    'evaluation.cascade = Some(cascade)',
    'evaluation.is_inactive()',
  ]);

  requireAll(diagnostics, files.planDagRuntimeRetry, sources.planDagRuntimeRetry, [
    'pub(super) async fn retry_failed_node_if_allowed',
    'plan_node_should_retry',
    'release_claim_if_recorded',
    'node.effective_retry_delay_ms()',
    'derive_plan_dag_claim_id',
    'ClaimAcquire::Acquired',
    'record_acquired_claim',
    'ClaimAcquire::Conflict',
    'record_compat_claim',
    'spawn_dispatch_attempt',
  ]);

  requireAll(diagnostics, files.planDagRuntimeSkips, sources.planDagRuntimeSkips, [
    'pub(super) async fn materialize_tainted_pending_skips',
    'pub(super) async fn force_skip_fail_fast_pending',
    'collect_tainted_pending',
    'pending_ids',
    'NodeState::SkippedUpstreamFailed',
    'NodeState::SkippedFailFastAbort',
    'emit_evidence_skipped',
  ]);

  requireAll(diagnostics, files.planDagRuntimeSpawn, sources.planDagRuntimeSpawn, [
    'pub(super) async fn spawn_dispatch_attempt',
    'NodeLifecycle::Running',
    'emit_evidence_running',
    'dispatch_node',
    'join_set.spawn',
    'task_contract_ctx.clone()',
  ]);

  requireAll(diagnostics, files.planDagRuntimeSuccess, sources.planDagRuntimeSuccess, [
    'pub(super) async fn record_successful_dispatch',
    'evaluate_success_acceptance',
    'release_claim_if_recorded',
    'evaluate_and_emit_rollback',
    'propagate_taint',
    'NodeResult',
    'retry_skipped_non_retryable: false',
    'FAILURE_POLICY_FAIL_FAST',
  ]);

  requireAll(diagnostics, files.planDagParser, sources.planDagParser, [
    'mod scanner;',
    'mod types;',
    'mod validation;',
    'pub(super) use scanner::parse_plan_dag;',
    'pub(super) use validation::build_validated_dag;',
    'pub(in crate::handlers::knowledge) use types::{DagNode, ParsedDag};',
  ]);

  requireAll(diagnostics, files.planDagParserTypes, sources.planDagParserTypes, [
    'mod errors;',
    'mod node;',
    'pub(in crate::handlers::knowledge::plan_dag) use errors::DagBuildError;',
    'pub(in crate::handlers::knowledge) use node::{DagNode, ParsedDag};',
    'pub(in crate::handlers::knowledge::plan_dag) use node::{',
    'pub(super) use node::{FAILURE_POLICY_CONTINUE, VALID_TARGETS};',
  ]);

  requireAll(diagnostics, files.planDagParserTypesNode, sources.planDagParserTypesNode, [
    'pub(in crate::handlers::knowledge) struct DagNode',
    'pub(in crate::handlers::knowledge::plan_dag) enum ReviewGateKind',
    'pub(in crate::handlers::knowledge) struct ParsedDag',
    'pub(in crate::handlers::knowledge::plan_dag::parser) const VALID_TARGETS',
    'pub(in crate::handlers::knowledge::plan_dag) const MAX_NODE_ATTEMPTS_CAP',
    'pub(in crate::handlers::knowledge::plan_dag) const MAX_RETRY_DELAY_MS',
    'pub(in crate::handlers::knowledge::plan_dag) fn acceptance_mode_kind',
    'pub(in crate::handlers::knowledge::plan_dag) fn has_acceptance_fan_in',
    'AcceptanceRequires::parse',
  ]);

  requireAll(diagnostics, files.planDagParserTypesErrors, sources.planDagParserTypesErrors, [
    'pub(in crate::handlers::knowledge::plan_dag) enum DagBuildError',
    'pub(in crate::handlers::knowledge::plan_dag) fn into_tool_result',
    'error_codes::INVALID_PARAM',
    'DagBuildError::NoNodes',
    'DagBuildError::InvalidTarget',
    'DagBuildError::InvalidRetryHint',
    'DagBuildError::AcceptanceFanInRequiresMissing',
    'DagBuildError::CompensateDirectionMismatch',
    'VALID_TARGETS',
  ]);

  requireAll(diagnostics, files.planDagParserScanner, sources.planDagParserScanner, [
    'mod keyword_pairs;',
    'mod lists;',
    'mod node_form;',
    'mod top_level;',
    'pub(in crate::handlers::knowledge::plan_dag) use top_level::parse_plan_dag;',
  ]);

  requireAll(diagnostics, files.planDagParserScannerTopLevel, sources.planDagParserScannerTopLevel, [
    'pub(in crate::handlers::knowledge::plan_dag) fn parse_plan_dag',
    'pub(super) fn scan_top_level_forms',
    'pub(super) fn top_form_head',
    'unsupported_top_forms.push(form)',
    'parse_node_form(&form)',
  ]);

  requireAll(diagnostics, files.planDagParserScannerNodeForm, sources.planDagParserScannerNodeForm, [
    'pub(super) fn parse_node_form',
    'scan_keyword_pairs(form)',
    'parse_id_list(&value)',
    '"target" | "target-tool" | "tool"',
    '"objective" => set_first(&mut objective, &value)',
    '"timeout-ms" | "timeout_ms"',
    '"target-project" | "target_project" | "project"',
    '"requested-cwd" | "requested_cwd" | "cwd"',
    '"acceptance-commands" | "acceptance_commands"',
    '"workstation-dispatch" | "workstation_dispatch"',
    'AcceptanceMode::parse',
    'AcceptanceRequires::parse',
    'RollbackPolicy::parse',
    'RollbackCascadeMode::parse',
    'unsupported_fields.push',
    'DagNode {',
  ]);

  requireAll(diagnostics, files.planDagParserScannerLists, sources.planDagParserScannerLists, [
    'pub(super) fn parse_id_list',
    'strip_prefix',
    'let mut esc = false',
    'out.push(s)',
  ]);

  requireAll(
    diagnostics,
    files.planDagParserScannerKeywordPairs,
    sources.planDagParserScannerKeywordPairs,
    [
      'pub(super) fn scan_keyword_pairs',
      'let mut in_string = false',
      'let key: String',
      'out.push((key, value))',
    ],
  );

  requireAll(diagnostics, files.planDagParserValidation, sources.planDagParserValidation, [
    'pub(in crate::handlers::knowledge::plan_dag) fn build_validated_dag',
    'fn kahn_topo_sort',
    'AcceptanceRequires::EvidenceKeys',
    'VALID_TARGETS.contains',
    'compute_transitive_ancestors',
  ]);

  requireAll(diagnostics, files.planDagAcceptance, sources.planDagAcceptance, [
    'mod evaluator;',
    'mod fan_in;',
    'mod payload;',
    'mod pause;',
    'mod types;',
    'pub(super) use evaluator::evaluate_node_acceptance;',
    'pub(super) use fan_in::apply_acceptance_fan_in;',
    'pub(super) use pause::derive_acceptance_pause_id;',
    'pub(super) use types::{',
  ]);

  requireAll(diagnostics, files.planDagAcceptanceTypes, sources.planDagAcceptanceTypes, [
    'pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceMode',
    'pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceRequires',
    'pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceStatus',
    'pub(in crate::handlers::knowledge::plan_dag) struct AcceptanceEvaluation',
    'pub(in crate::handlers::knowledge::plan_dag) struct AcceptanceFanInOutcome',
    'pub(in crate::handlers::knowledge::plan_dag) fn as_wire',
    'pub(in crate::handlers::knowledge::plan_dag) fn parse',
    'pub(in crate::handlers::knowledge::plan_dag) fn is_inactive',
    'pub(in crate::handlers::knowledge::plan_dag) fn to_json',
  ]);

  requireAll(
    diagnostics,
    files.planDagAcceptanceEvaluator,
    sources.planDagAcceptanceEvaluator,
    [
      'pub(in crate::handlers::knowledge::plan_dag) fn evaluate_node_acceptance',
      'AcceptanceMode::parse',
      'AcceptanceStatus::ManualRequired',
      'AcceptanceStatus::Rejected',
      'inner_payload_failure_signal',
      'inner_payload_missing_keys',
      'split_lisp_string_list',
    ],
  );

  requireAll(diagnostics, files.planDagAcceptanceFanIn, sources.planDagAcceptanceFanIn, [
    'pub(in crate::handlers::knowledge::plan_dag) fn apply_acceptance_fan_in',
    'AcceptanceRequires::AllSucceeded',
    'AcceptanceRequires::AnySucceeded',
    'AcceptanceRequires::EvidenceKeys',
    'inner_payload_missing_keys',
    'AcceptanceFanInOutcome',
  ]);

  requireAll(diagnostics, files.planDagAcceptancePayload, sources.planDagAcceptancePayload, [
    'pub(super) fn inner_payload_failure_signal',
    'pub(super) fn inner_payload_missing_keys',
    'fn inner_payload_contains_key',
    'workstation_dispatch_status',
    'typed_evidence',
  ]);

  requireAll(diagnostics, files.planDagAcceptancePause, sources.planDagAcceptancePause, [
    'pub(in crate::handlers::knowledge::plan_dag) fn derive_acceptance_pause_id',
    'acceptance:plan:{}:v{}:{}',
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
    'mod runner;',
    'mod task_contract_ctx;',
    'mod types;',
    'mod workstation;',
    'pub(super) use runner::dispatch_node',
    'pub(super) use task_contract_ctx::TaskContractDispatchCtx',
    'pub(super) use types::DispatchOutcome',
  ]);

  requireAll(diagnostics, files.planDagDispatchTypes, sources.planDagDispatchTypes, [
    'pub(in crate::handlers::knowledge::plan_dag) struct DispatchOutcome',
    'node_id: String',
    'inner_payload: Value',
    'classification: std::result::Result<(), String>',
    'non_retryable: bool',
  ]);

  requireAll(
    diagnostics,
    files.planDagDispatchWorkstation,
    sources.planDagDispatchWorkstation,
    [
      'pub(in crate::handlers::knowledge::plan_dag) fn node_to_workstation_hints',
      'pub(in crate::handlers::knowledge::plan_dag) fn workstation_outcome_to_dispatch_pair',
      'workstation_dispatch::WorkstationDispatchHints',
      'plan::split_lisp_string_list',
      'workstation_dispatch::outcome_to_response_fields',
      'WorkstationDispatchOutcome::SafeDescriptor',
    ],
  );

  requireAll(
    diagnostics,
    files.planDagDispatchTaskContractCtx,
    sources.planDagDispatchTaskContractCtx,
    [
      'pub(in crate::handlers::knowledge::plan_dag) struct TaskContractDispatchCtx',
      'pub mode: plan::TaskContractEmitMode',
      'pub dispatch_contract_mode: plan::DispatchContractMode',
      'fn off',
      'fn from_args',
      'plan::parse_task_contract_emit_mode',
      'plan::parse_dispatch_contract_mode',
    ],
  );

  requireAll(diagnostics, files.planDagDispatchRunner, sources.planDagDispatchRunner, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn dispatch_node',
    'workstation_dispatch::run_workstation_dispatch_with_contract',
    'agent_execution::handle',
    'task_delegate::handle',
    'flow_run::handle',
    'plan::emit_task_contract',
    'plan::merge_task_contract_block',
    'tool_result_payload',
    'build_node_inner_args',
    'node_to_workstation_hints',
    'workstation_outcome_to_dispatch_pair',
  ]);

  requireAll(diagnostics, files.planDagRollback, sources.planDagRollback, [
    'mod cascade;',
    'mod descriptor;',
    'mod run;',
    'mod types;',
    'pub(super) use cascade::{',
    'pub(super) use descriptor::{',
    'pub(super) use run::{',
    'pub(super) use types::{',
    'use super::DagNode',
  ]);

  requireAll(diagnostics, files.planDagRollbackTypes, sources.planDagRollbackTypes, [
    'mod cascade;',
    'mod evaluation;',
    'mod node_ext;',
    'mod policy;',
    'pub(in crate::handlers::knowledge::plan_dag) use cascade::{',
    'pub(in crate::handlers::knowledge::plan_dag) use evaluation::{',
    'pub(in crate::handlers::knowledge::plan_dag) use policy::RollbackPolicy;',
  ]);

  requireAll(
    diagnostics,
    files.planDagRollbackTypesNodeExt,
    sources.planDagRollbackTypesNodeExt,
    [
      'impl DagNode',
      'fn rollback_policy_kind',
      'fn rollback_cascade_kind',
      'fn has_active_rollback_cascade',
      'fn has_rollback_hints',
      'RollbackPolicy::parse',
      'RollbackCascadeMode::parse',
    ],
  );

  requireAll(diagnostics, files.planDagRollbackTypesPolicy, sources.planDagRollbackTypesPolicy, [
    'pub(in crate::handlers::knowledge::plan_dag) enum RollbackPolicy',
    'RollbackPolicy::None',
    'RollbackPolicy::Descriptor',
    'RollbackPolicy::Workstation',
    'fn as_wire',
    'fn parse',
  ]);

  requireAll(
    diagnostics,
    files.planDagRollbackTypesEvaluation,
    sources.planDagRollbackTypesEvaluation,
    [
      'pub(in crate::handlers::knowledge::plan_dag) enum RollbackStatus',
      'pub(in crate::handlers::knowledge::plan_dag) struct RollbackEvaluation',
      'RollbackStatus::NotRequested',
      'RollbackStatus::DescriptorReady',
      'RollbackStatus::Dispatched',
      'RollbackStatus::Refused',
      'RollbackStatus::Failed',
      'fn is_inactive',
      'fn to_json',
      'cascade.to_json()',
    ],
  );

  requireAll(diagnostics, files.planDagRollbackTypesCascade, sources.planDagRollbackTypesCascade, [
    'pub(in crate::handlers::knowledge::plan_dag) enum RollbackCascadeMode',
    'pub(in crate::handlers::knowledge::plan_dag) struct CascadeCompensationOutcome',
    'pub(in crate::handlers::knowledge::plan_dag) struct CascadeRollbackOutcome',
    'RollbackCascadeMode::DispatchSafe',
    'RollbackStatus',
    'fn is_inactive',
    'fn to_json',
    'compensations',
  ]);

  requireAll(diagnostics, files.planDagRollbackDescriptor, sources.planDagRollbackDescriptor, [
    'pub(in crate::handlers::knowledge::plan_dag) fn build_rollback_descriptor',
    'pub(in crate::handlers::knowledge::plan_dag) struct RollbackDescriptor',
    'pub(in crate::handlers::knowledge::plan_dag) fn to_workstation_hints',
    'pub(in crate::handlers::knowledge::plan_dag) fn safety_check_for_workstation',
    'pub(in crate::handlers::knowledge::plan_dag) fn pre_dispatch_rollback_decision',
    'rollback workstation dispatch requires :rollback-objective',
    'workstation_dispatch::INFERABLE_DISPATCH_STRATEGIES',
  ]);

  requireAll(diagnostics, files.planDagRollbackRun, sources.planDagRollbackRun, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn run_rollback',
    'pub(in crate::handlers::knowledge::plan_dag) fn truncate_rollback_brief_preview',
    'workstation_dispatch::run_workstation_dispatch',
    'WorkstationDispatchOutcome::Dispatched',
    'WorkstationDispatchOutcome::DryRun',
    'WorkstationDispatchOutcome::InnerError',
    'WorkstationDispatchOutcome::SafeDescriptor',
  ]);

  requireAll(diagnostics, files.planDagRollbackCascade, sources.planDagRollbackCascade, [
    'mod dispatch_outcome;',
    'mod ordering;',
    'mod plan_entry;',
    'mod runner;',
    'pub(in crate::handlers::knowledge::plan_dag) use ordering::compute_compensation_order',
    'pub(in crate::handlers::knowledge::plan_dag) use plan_entry::build_compensation_plan_entry',
    'pub(in crate::handlers::knowledge::plan_dag) use runner::run_cascade_rollback',
  ]);

  requireAll(
    diagnostics,
    files.planDagRollbackCascadeOrdering,
    sources.planDagRollbackCascadeOrdering,
    [
      'pub(in crate::handlers::knowledge::plan_dag) fn compute_compensation_order',
      'HashMap',
      'HashSet',
      'rollback_after',
      'compensate_node',
    ],
  );

  requireAll(
    diagnostics,
    files.planDagRollbackCascadePlanEntry,
    sources.planDagRollbackCascadePlanEntry,
    [
      'pub(in crate::handlers::knowledge::plan_dag) fn build_compensation_plan_entry',
      'build_rollback_descriptor',
      'to_workstation_hints',
      'workstation_dispatch::build_task_brief',
      'RollbackStatus::DescriptorReady',
    ],
  );

  requireAll(
    diagnostics,
    files.planDagRollbackCascadeRunner,
    sources.planDagRollbackCascadeRunner,
    [
      'pub(in crate::handlers::knowledge::plan_dag) async fn run_cascade_rollback',
      'compute_compensation_order',
      'build_compensation_plan_entry',
      'map_dispatch_outcome_to_compensation',
      'RollbackCascadeMode::DispatchSafe',
      'workstation_dispatch::run_workstation_dispatch',
    ],
  );

  requireAll(
    diagnostics,
    files.planDagRollbackCascadeDispatchOutcome,
    sources.planDagRollbackCascadeDispatchOutcome,
    [
      'pub(super) fn map_dispatch_outcome_to_compensation',
      'WorkstationDispatchOutcome',
      'O::Dispatched',
      'O::DryRun',
      'O::InnerError',
      'O::SafeDescriptor',
      'truncate_rollback_brief_preview',
    ],
  );

  requireAll(diagnostics, files.planDagResume, sources.planDagResume, [
    'mod action;',
    'mod evidence;',
    'mod listener;',
    'mod validation;',
    'pub(in crate::handlers::knowledge) use action::action_execute_resume;',
    'pub(crate) use listener::{handle_review_resolved_plan_node_event, PlanNodeResumeListenerOutcome};',
    'pub(in crate::handlers::knowledge) use validation::{validate_resume_request, PlanNodeResumeError};',
  ]);
  requireAll(
    diagnostics,
    files.planDagResumeValidation,
    sources.planDagResumeValidation,
    [
    'pub(in crate::handlers::knowledge) enum PlanNodeResumeError',
    'pub(in crate::handlers::knowledge) fn validate_resume_request',
      'pub(in crate::handlers::knowledge::plan_dag) fn code',
      'pub(in crate::handlers::knowledge::plan_dag) fn message',
      'ParsedReviewQuestionId',
      'derive_plan_node_topic_hash',
      'is_plan_node_review_action',
      'ReviewGateKind::QuestionEvent',
    ],
  );
  requireAll(diagnostics, files.planDagResumeAction, sources.planDagResumeAction, [
    'pub(in crate::handlers::knowledge) async fn action_execute_resume',
    'fn resume_error_to_tool_result',
    'parse_review_question_id_struct',
    'TaskContractDispatchCtx::from_args',
    'resume_dispatched',
    'resume_failed',
    'bus_publish_warnings',
  ]);
  requireAll(
    diagnostics,
    files.planDagResumeEvidence,
    sources.planDagResumeEvidence,
    [
    'async fn emit_resume_decision_evidence',
      'publish_plan_node_state_change',
      'paused ->',
      'EvidenceEntry::new',
      'PLAN_DAG_NODE_DISPATCH',
      'review_resume',
    ],
  );
  requireAll(
    diagnostics,
    files.planDagResumeListener,
    sources.planDagResumeListener,
    [
    'pub(crate) enum PlanNodeResumeListenerOutcome',
    'pub(crate) async fn handle_review_resolved_plan_node_event',
      'ArtifactIdNotUuid',
      'ValidationRejected',
      'TaskContractDispatchCtx::off()',
      'PlanNodeResumeInput',
      'ReviewDecision::Approved',
    ],
  );

  requireAll(diagnostics, files.planDagOutcome, sources.planDagOutcome, [
    'mod execution;',
    'mod node_result;',
    'mod state;',
    'pub(super) use execution::ExecutionOutcome;',
    'pub(super) use node_result::NodeResult;',
    'pub(super) use state::{NodeLifecycle, NodeState};',
  ]);
  requireAll(diagnostics, files.planDagOutcomeState, sources.planDagOutcomeState, [
    'pub(in crate::handlers::knowledge::plan_dag) enum NodeState',
    'pub(in crate::handlers::knowledge::plan_dag) enum NodeLifecycle',
    'SkippedUpstreamFailed',
    'SkippedCondition',
    'SkippedFailFastAbort',
    'Paused',
  ]);
  requireAll(
    diagnostics,
    files.planDagOutcomeNodeResult,
    sources.planDagOutcomeNodeResult,
    [
      'pub(in crate::handlers::knowledge::plan_dag) struct NodeResult',
      'pub(in crate::handlers::knowledge::plan_dag) fn skipped',
      'impl Default for NodeResult',
      'Value::Null',
      'attempts_made',
      'retry_skipped_non_retryable',
      'AcceptanceEvaluation',
      'RollbackEvaluation',
    ],
  );
  requireAll(
    diagnostics,
    files.planDagOutcomeExecution,
    sources.planDagOutcomeExecution,
    [
      'pub(in crate::handlers::knowledge::plan_dag) struct ExecutionOutcome',
      'pub(in crate::handlers::knowledge::plan_dag) fn node_results_json',
      'pub(in crate::handlers::knowledge::plan_dag) fn paused_nodes_json',
      'pub(in crate::handlers::knowledge::plan_dag) fn skipped_nodes_json',
      'pub(in crate::handlers::knowledge::plan_dag) fn aggregate_status',
      'pub(in crate::handlers::knowledge::plan_dag) fn runner_status',
      'pub(in crate::handlers::knowledge::plan_dag) fn target_plan_status',
      'review_question_warning',
      'retry_skipped_non_retryable',
      'PlanStatus::Succeeded',
      'PlanStatus::Failed',
    ],
  );

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
    'pub(super) async fn maybe_run_distill_trigger',
    'super::super::workflow::handle',
    'tool_result_payload',
    'distill_invoked_ok',
    'distill_invoked_handler_error',
  ]);

  requireAll(diagnostics, files.planDagLifecycle, sources.planDagLifecycle, [
    'mod claims;',
    'mod context;',
    'mod event_ref;',
    'mod finalize;',
    'mod nodes;',
    'mod retry;',
    'mod review;',
    'pub(super) use claims::*;',
    'pub(super) use context::EvidenceCtx;',
    'pub(super) use event_ref::publish_plan_node_state_change;',
    'pub(super) use finalize::emit_evidence_dag_finalized;',
    'pub(super) use nodes::{',
    'pub(super) use retry::{',
    'pub(super) use review::emit_paused_review_gate;',
  ]);

  requireAll(diagnostics, files.planDagLifecycleContext, sources.planDagLifecycleContext, [
    'pub(in crate::handlers::knowledge::plan_dag) struct EvidenceCtx',
    'plan_id: uuid::Uuid',
    'plan_version: i32',
    'target_project_arg',
  ]);

  requireAll(diagnostics, files.planDagLifecycleEventRef, sources.planDagLifecycleEventRef, [
    'pub(in crate::handlers::knowledge::plan_dag) const EVENT_REF_SOURCE_EXECUTION',
    'pub(in crate::handlers::knowledge::plan_dag) const EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED',
    'pub(in crate::handlers::knowledge::plan_dag) fn deterministic_plan_node_event_id',
    'pub(in crate::handlers::knowledge::plan_dag) fn build_plan_node_state_changed_event',
    'pub(in crate::handlers::knowledge::plan_dag) async fn publish_plan_node_state_change',
    'ExecutionEvent::PlanNodeStateChanged',
    'EventRef::new(',
    'lookup_or_query_plan_node_state_change',
  ]);

  requireAll(diagnostics, files.planDagLifecycleFinalize, sources.planDagLifecycleFinalize, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_dag_finalized',
    'with_state_transition("dag_finalized")',
    'with_extra("event_kind", json!("plan_dag_finalized"))',
    'with_extra("distill"',
  ]);

  requireAll(diagnostics, files.planDagLifecycleNodes, sources.planDagLifecycleNodes, [
    'mod acceptance;',
    'mod finished;',
    'mod rollback;',
    'mod running;',
    'mod skipped;',
    'pub(in crate::handlers::knowledge::plan_dag) use acceptance::emit_evidence_acceptance;',
    'pub(in crate::handlers::knowledge::plan_dag) use finished::emit_evidence_finished;',
    'pub(in crate::handlers::knowledge::plan_dag) use rollback::emit_evidence_rollback;',
    'pub(in crate::handlers::knowledge::plan_dag) use running::emit_evidence_running;',
    'pub(in crate::handlers::knowledge::plan_dag) use skipped::emit_evidence_skipped;',
  ]);

  requireAll(diagnostics, files.planDagLifecycleNodesRunning, sources.planDagLifecycleNodesRunning, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_running',
    'with_state_transition("ready -> running")',
    'publish_plan_node_state_change',
    'with_primary_event_ref',
  ]);

  requireAll(diagnostics, files.planDagLifecycleNodesFinished, sources.planDagLifecycleNodesFinished, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_finished',
    'with_state_transition("running -> succeeded")',
    'with_state_transition("running -> failed")',
    'with_inner_dispatch(inner_payload.clone())',
    'with_extra("inner_error", inner_payload.clone())',
  ]);

  requireAll(diagnostics, files.planDagLifecycleNodesRollback, sources.planDagLifecycleNodesRollback, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_rollback',
    'failed -> rollback_',
    'rollback_policy',
    'rollback_cascade',
    'rollback_inner_result',
  ]);

  requireAll(diagnostics, files.planDagLifecycleNodesAcceptance, sources.planDagLifecycleNodesAcceptance, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_acceptance',
    'succeeded -> acceptance_',
    'acceptance_fan_in',
    'acceptance_pause_id',
    'derive_acceptance_pause_id',
  ]);

  requireAll(diagnostics, files.planDagLifecycleNodesSkipped, sources.planDagLifecycleNodesSkipped, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_skipped',
    'with_state_transition("pending -> skipped")',
    'skip_reason',
    'PLAN_NODE_DEFAULT_ATTEMPT',
  ]);

  requireAll(diagnostics, files.planDagLifecycleRetry, sources.planDagLifecycleRetry, [
    'pub(in crate::handlers::knowledge::plan_dag) const PLAN_NODE_DEFAULT_ATTEMPT',
    'pub(in crate::handlers::knowledge::plan_dag) fn plan_node_should_retry',
    'saturating_sub(current_attempt)',
  ]);

  requireAll(diagnostics, files.planDagLifecycleReview, sources.planDagLifecycleReview, [
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_paused_review_gate',
    'publish_question(ev)',
    'derive_plan_node_review_question_id',
    'with_state_transition("pending -> paused")',
    'review_question_warning',
  ]);

  requireAll(diagnostics, files.planDagLifecycleClaims, sources.planDagLifecycleClaims, [
    'use super::{publish_plan_node_state_change, EvidenceCtx};',
    'use super::super::claim_lease::PlanDagClaim;',
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claimed',
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claim_released',
    'pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claim_conflict',
    'with_state_transition("pending -> claimed")',
    'with_state_transition("claimed -> released")',
    'with_state_transition("pending -> failed")',
    'claim_conflict',
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
    '.missiond/v3/runtime/plans/<plan_id>.evidence.json',
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

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden contract text present: ${needle}` });
    }
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
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring/artifact.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring/validation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/approve.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/mark.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/supersede.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/mode.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/evidence.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/rules.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/llm.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply/persisted.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/bridge.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/internal.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/workstation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/internal_dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/distill_chain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/dispatch_response.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/manifest.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/projection.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/acceptance.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/bookkeeping.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/claiming.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/claims.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/drain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/failures.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/gates.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/rollbacks.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/retry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/skips.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/spawn.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/success.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types/node.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types/errors.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/top_level.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/node_form.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/lists.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/keyword_pairs.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/validation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/evaluator.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/fan_in.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/payload.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/pause.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/workstation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/task_contract_ctx.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/runner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/node_ext.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/policy.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/evaluation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/cascade.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/ordering.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/plan_entry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/runner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/dispatch_outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/validation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/action.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/evidence.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/listener.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/state.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/node_result.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/execution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/projection.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/finalization.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/event_ref.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/finalize.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/running.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/finished.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/rollback.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/acceptance.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/skipped.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/retry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/claims.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/scheduler.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/mode.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/tests.rs"]
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold; plan/compile_authoring.rs owns mission_plan plan-authoring entry/core; plan/compile_authoring/artifact.rs owns plan file-first artifact egress; plan/compile_authoring/validation.rs owns planner prompt and Lisp output validation; plan/approval_review.rs owns mission_plan plan-review-gate caller action facade plus shared PLAN_REVIEW_ACTIONS wiring. plan/approval_review/approve.rs owns mission_plan approve action: action_approve, action_approve_with_resolution, plan_action_approve_with_policy_only, PlanStatus::Approved transition, review-resolution validation, and resolved-event egress. plan/approval_review/mark.rs owns mission_plan mark action: action_mark, action_mark_with_resolution, plan_action_mark_with_policy_only, target_raw parsing, mark-to-approved policy auto-promotion, and resolved-event egress. plan/approval_review/supersede.rs owns mission_plan supersede action: action_supersede, action_supersede_with_resolution, plan_action_supersede_with_policy_only, destructive-action refusal, PlanStatus::Superseded transition, and resolved-event egress. plan/approval_review/proposer.rs owns mission_plan plan-review LLM proposal helpers: build_plan_automation_ctx, request_plan_auto_approve_proposal, attach_plan_proposal_block, attach_plan_apply_gate_block, parse_plan_proposer_mode_or_error, and plan_proposer_summary keep propose-only audit blocks outside the caller action facade. plan/approval_review/subscriber.rs owns mission_plan plan-review subscriber bridge: PlanSubscriberOutcome and handle_review_resolved_event keep approval/rejection/needs_changes transitions tied to the same review envelope validation without bloating the caller action facade. plan/field_inference.rs owns mission_plan execute preflight field inference/core; plan/field_inference/mode.rs owns infer_plan_fields/workstation_inference_mode parsing and DAG preflight gates; plan/field_inference/evidence.rs owns evidence-sidecar scanners; plan/field_inference/rules.rs owns deterministic field inference rules; plan/field_inference/llm.rs owns Sonnet proposal parsing, validation, conflict reconciliation, prompt construction, gateway request, and recent evidence reads for inference; plan/field_inference/apply.rs owns apply_gate, including explicit apply approval, LLM caller approval, and response block splicing; plan/field_inference/apply/persisted.rs owns persisted_apply, including proposal-hash preflight, PLAN.lisp persisted annotation synthesis, evidence entry construction, and response block splicing; plan/execution_runtime.rs owns mission_plan execute facade orchestration; plan/execution_runtime/bridge.rs owns bridge descriptor projection; plan/execution_runtime/internal.rs owns mission_plan internal dispatch runtime and PLAN_RUNNER_EVENT_REF_UNAVAILABLE_REASON; plan/execution_runtime/workstation.rs owns workstation proposal/auto-spawn execution adjuncts; plan/internal_dispatch.rs owns mission_plan inner target argument projection; plan/execute_hints.rs owns mission_plan PLAN.lisp hint parsing; plan/task_contract.rs owns mission_plan task-contract Lisp projection; plan/distill_chain.rs owns mission_plan cross-plan distill-chain egress; plan/dispatch_response.rs owns mission_plan execution response egress; plan/evidence_sidecar.rs owns mission_plan evidence sidecar egress; plan/router_policy_dry_run.rs owns the mission_plan router-policy adapter; plan/router_policy_dry_run/predicate.rs owns router-policy predicate projection; plan/router_policy_dry_run/readiness.rs owns router-policy trace-index/backend-readiness projection; plan/router_policy_dry_run/descriptor.rs owns router dispatch descriptor projection; plan/router_policy_dry_run/schema_parser.rs owns the router-policy Lisp schema parser shared by the policy and backend-registry advisory projections; plan/task_runner_dry_run.rs owns the mission_plan task-runner adapter; plan/task_runner_dry_run/manifest.rs owns task-runner manifest loading/parsing; plan/task_runner_dry_run/projection.rs owns task-runner manifest response projection; plan/tests.rs holds the historical mission_plan regression suite outside the runtime facade; plan_dag/runtime.rs owns the DAG live runtime wave loop; plan_dag/runtime/acceptance.rs owns DAG runtime success acceptance projection; plan_dag/runtime/bookkeeping.rs owns DAG runtime bookkeeping; plan_dag/runtime/claiming.rs owns DAG runtime dispatch claim preparation; plan_dag/runtime/claims.rs owns DAG runtime claim acquisition and release projection; plan_dag/runtime/drain.rs owns DAG runtime wave drain projection; plan_dag/runtime/failures.rs owns DAG runtime final failure projection; plan_dag/runtime/gates.rs owns DAG runtime ready-node gate filtering; plan_dag/runtime/rollbacks.rs owns DAG runtime rollback evaluation; plan_dag/runtime/retry.rs owns DAG runtime retry projection; plan_dag/runtime/skips.rs owns DAG runtime skip materialization; plan_dag/runtime/spawn.rs owns DAG runtime dispatch spawn projection; plan_dag/runtime/success.rs owns DAG runtime successful dispatch projection; plan_dag/parser.rs is the DAG parser/validator facade; plan_dag/parser/types.rs is the DAG parser types facade; plan_dag/parser/types/node.rs owns DAG node shapes and typed hint projections; plan_dag/parser/types/errors.rs owns DAG build error egress; plan_dag/parser/scanner.rs is the DAG scanner facade; plan_dag/parser/scanner/top_level.rs owns top-level PLAN.lisp S-expression scanning; plan_dag/parser/scanner/node_form.rs owns node form keyword lowering; plan_dag/parser/scanner/lists.rs owns DAG id-list parsing; plan_dag/parser/scanner/keyword_pairs.rs owns Lisp keyword/value token scanning; plan_dag/parser/validation.rs owns DAG contract validation/topological ordering; plan_dag/acceptance.rs is the DAG acceptance facade; plan_dag/acceptance/types.rs owns typed acceptance contracts; plan_dag/acceptance/evaluator.rs owns per-node acceptance evaluation; plan_dag/acceptance/fan_in.rs owns cross-node acceptance fan-in; plan_dag/acceptance/payload.rs owns inner-payload signal/key scanning; plan_dag/acceptance/pause.rs owns deterministic acceptance pause ids; plan_dag/claim_lease.rs owns the DAG claim/lease core; plan_dag/dispatch.rs is the DAG node dispatch facade; plan_dag/dispatch/types.rs owns DAG dispatch outcome shape; plan_dag/dispatch/workstation.rs owns workstation hint/outcome projection; plan_dag/dispatch/task_contract_ctx.rs owns per-run task-contract dispatch context; plan_dag/dispatch/runner.rs owns DAG node dispatch execution; plan_dag/rollback.rs is the DAG rollback facade; plan_dag/rollback/types.rs is the rollback types facade; plan_dag/rollback/types/node_ext.rs owns DagNode rollback hint projections; plan_dag/rollback/types/policy.rs owns rollback policy parsing; plan_dag/rollback/types/evaluation.rs owns node-local rollback evaluation JSON; plan_dag/rollback/types/cascade.rs owns cascade rollback outcome JSON; plan_dag/rollback/descriptor.rs owns rollback descriptor and pre-dispatch safety; plan_dag/rollback/run.rs owns node-local rollback execution; plan_dag/rollback/cascade.rs is the DAG cascade rollback facade; plan_dag/rollback/cascade/ordering.rs owns compensation ordering; plan_dag/rollback/cascade/plan_entry.rs owns plan-mode compensation projection; plan_dag/rollback/cascade/runner.rs owns dispatch-safe cascade execution; plan_dag/rollback/cascade/dispatch_outcome.rs owns workstation-dispatch outcome mapping; plan_dag/resume.rs is the DAG review-resume facade; plan_dag/resume/validation.rs owns resume id validation and error vocabulary; plan_dag/resume/action.rs owns explicit resume action entry/egress; plan_dag/resume/evidence.rs owns resume decision evidence; plan_dag/resume/listener.rs owns bus-resolved plan-node resume bridge; plan_dag/outcome.rs is the DAG node outcome facade; plan_dag/outcome/state.rs owns node state/lifecycle enums; plan_dag/outcome/node_result.rs owns node result shape/defaults; plan_dag/outcome/execution.rs owns DAG execution outcome response projection; plan_dag/projection.rs owns the DAG response projection core; plan_dag/finalization.rs owns the DAG finalization projection core; plan_dag/lifecycle.rs is the DAG lifecycle facade; plan_dag/lifecycle/context.rs owns per-run evidence context; plan_dag/lifecycle/event_ref.rs owns deterministic event refs and bus publish fallback; plan_dag/lifecycle/finalize.rs owns dag_finalized evidence rows; plan_dag/lifecycle/nodes.rs is the DAG node evidence row facade; plan_dag/lifecycle/nodes/running.rs owns ready->running evidence rows; plan_dag/lifecycle/nodes/finished.rs owns running->finished evidence rows; plan_dag/lifecycle/nodes/rollback.rs owns failed->rollback evidence rows; plan_dag/lifecycle/nodes/acceptance.rs owns succeeded->acceptance evidence rows; plan_dag/lifecycle/nodes/skipped.rs owns pending->skipped evidence rows; plan_dag/lifecycle/retry.rs owns retry attempt constants and retry predicate; plan_dag/lifecycle/review.rs owns paused review-gate evidence rows; plan_dag/lifecycle/claims.rs owns the DAG claim lifecycle evidence rows; plan_dag/scheduler.rs owns the DAG scheduler projection core; plan_dag/mode.rs owns the DAG scheduler-mode gate; plan_dag/tests.rs does the same for the DAG scheduler regression suite; execute can derive target_source=plan_hint from plan.sexp_text. DAG execution parses node-local Lisp hints."))
  (compression-contract
    :checks ["node scripts/check-v3-plan-execution-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.planHandler, `
use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
pub(super) fn load_sonnet_compiler_model() {
  let router_config = RouterRuntimeConfig::load_for_current_dir().unwrap();
  let _ = "V3_BLUEPRINT_CONFIG_ERROR";
  let _ = router_config.queued_sonnet_model;
}
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
parse_plan_hints(&plan.sexp_text);
let x = (t, "plan_hint");
let target_source = "";
let dispatch_strategy_source = "";
super::super::plan_dag::action_execute_dag_v1;
mod bridge;
pub(super) use bridge::{action_execute_bridge, attach_inference_block};
mod internal;
pub(super) use internal::action_execute_internal;
mod workstation;
pub(super) use workstation::{
  attach_workstation_auto_spawn_gate_block,
  attach_workstation_proposals_block,
  compute_workstation_auto_spawn_gate,
  compute_workstation_proposal_bundle,
  plan_hints_carry_workstation_signal,
};
`);
  writeFixture(root, DEFAULT_FILES.planExecutionBridge, `
pub(in crate::handlers::knowledge::plan) fn attach_inference_block() {}
pub(in crate::handlers::knowledge::plan) fn action_execute_bridge() {
  "bridge_ready";
  "runner_status": "bridge_only";
  next_call;
}
`);
  writeFixture(root, DEFAULT_FILES.planExecutionInternal, `
pub(in crate::handlers::knowledge::plan) async fn action_execute_internal() {}
const PLAN_RUNNER_EVENT_REF_UNAVAILABLE_REASON: &str = "without a live ExecutionEvent ref";
parse_task_contract_emit_mode;
validate_session_trace_path_arg;
workstation_dispatch::run_workstation_dispatch_with_contract_and_trace;
build_internal_dispatch_args;
agent_execution::handle;
task_delegate::handle;
flow_run::handle;
evidence_collector::append;
build_internal_dispatch_success_response;
`);
  writeFixture(root, DEFAULT_FILES.planExecutionWorkstation, `
pub(in crate::handlers::knowledge::plan) async fn compute_workstation_proposal_bundle() {}
pub(in crate::handlers::knowledge::plan) fn plan_hints_carry_workstation_signal() {}
pub(in crate::handlers::knowledge::plan) fn attach_workstation_proposals_block() {}
pub(in crate::handlers::knowledge::plan) async fn compute_workstation_auto_spawn_gate() {}
pub(in crate::handlers::knowledge::plan) fn attach_workstation_auto_spawn_gate_block() {}
WorkstationProposalGate;
request_workstation_proposals;
evaluate_workstation_auto_spawn_gate;
run_workstation_dispatch_with_contract;
workstation_auto_spawn_gate;
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
mod artifact;
mod validation;
pub(super) use artifact::{extract_plan_file_args, maybe_write_plan_artifact};
pub(super) use validation::{
  build_planner_system_prompt, build_planner_user_prompt, collect_string_list,
  parens_balanced, strip_fenced_code_block, top_level_head, validate_compiled_plan_sexp,
  SexpValidationError,
};
pub(super) fn resolve_dry_run_plan_target() { return Ok("mission_task_delegate"); }
pub(super) fn render_dry_run_plan_sexp() {
  let compiler_model = load_sonnet_compiler_model().unwrap();
  let _ = compiler_model.clone();
  String::from("(plan-draft\\n");
  ":execution-readiness :dry-run-executable-scaffold";
  push_lisp_string_field(&mut out, "target", input.target);
  push_lisp_string_field(&mut out, "objective", input.objective);
  out.push_str("  :nodes\\n");
}
`);
  writeFixture(root, DEFAULT_FILES.planCompileArtifact, `
pub(in crate::handlers::knowledge::plan) struct PlanFileArgs {}
pub(in crate::handlers::knowledge::plan) fn extract_plan_file_args() {}
pub(in crate::handlers::knowledge::plan) async fn maybe_write_plan_artifact() {
  attempt_artifact_write;
  ArtifactKind::Plan;
  outcome.splice_into(payload);
}
`);
  writeFixture(root, DEFAULT_FILES.planCompileValidation, `
pub(in crate::handlers::knowledge::plan) fn collect_string_list() {}
pub(in crate::handlers::knowledge::plan) fn build_planner_system_prompt() {}
pub(in crate::handlers::knowledge::plan) fn build_planner_user_prompt() {}
pub(in crate::handlers::knowledge::plan) struct SexpValidationError {}
pub(in crate::handlers::knowledge::plan) fn validate_compiled_plan_sexp() {
  ALLOWED_PLAN_HEADS;
  board_task_id;
}
pub(in crate::handlers::knowledge::plan) fn strip_fenced_code_block() {}
pub(in crate::handlers::knowledge::plan) fn parens_balanced() {}
pub(in crate::handlers::knowledge::plan) fn top_level_head() {}
`);
  writeFixture(root, DEFAULT_FILES.planApprovalReview, `
mod approve;
mod mark;
mod proposer;
mod subscriber;
mod supersede;
pub(super) use self::approve::action_approve;
pub(super) use self::mark::action_mark;
use self::proposer::{
    attach_plan_apply_gate_block, attach_plan_proposal_block, build_plan_automation_ctx,
    parse_plan_proposer_mode_or_error, plan_proposer_summary, request_plan_auto_approve_proposal,
};
pub(crate) use self::subscriber::{handle_review_resolved_event, PlanSubscriberOutcome};
pub(super) use self::supersede::action_supersede;
pub(super) const PLAN_REVIEW_ACTIONS: &[&str] = &["compile", "approve", "mark", "supersede"];
`);
  writeFixture(root, DEFAULT_FILES.planApprovalApprove, `
use super::*;
pub(super) async fn action_approve() {
  parse_review_resolution_input(args);
  maybe_emit_review_question_resolved;
  attach_plan_apply_gate_block;
}
async fn action_approve_with_resolution() {}
async fn plan_action_approve_with_policy_only() {
  PlanStatus::Approved;
}
`);
  writeFixture(root, DEFAULT_FILES.planApprovalMark, `
use super::*;
pub(super) async fn action_mark() {
  parse_review_resolution_input(args);
  maybe_emit_review_question_resolved;
  target_raw;
}
async fn action_mark_with_resolution() {}
async fn plan_action_mark_with_policy_only() {
  PlanStatus::Approved;
}
`);
  writeFixture(root, DEFAULT_FILES.planApprovalSupersede, `
use super::*;
pub(in crate::handlers::knowledge::plan) async fn action_supersede() {
  parse_review_resolution_input(args);
  maybe_emit_review_question_resolved;
  destructive;
}
async fn action_supersede_with_resolution() {}
async fn plan_action_supersede_with_policy_only() {
  PlanStatus::Superseded;
}
`);
  writeFixture(root, DEFAULT_FILES.planApprovalProposer, `
use super::*;
pub(super) fn build_plan_automation_ctx() {}
pub(super) async fn request_plan_auto_approve_proposal() {}
fn uses_v3_model() { load_sonnet_compiler_model(); }
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
mod mode;
pub(crate) use mode::{parse_infer_plan_fields_mode, InferPlanFieldsMode};
pub(super) use mode::{
  parse_workstation_inference_mode, refuse_workstation_inference_in_dag_mode,
};
pub(super) struct PlanFieldInference {}
mod evidence;
use evidence::*;
mod rules;
pub(super) use rules::{
  caller_bool, caller_str, caller_string_list, compute_plan_field_inference, PlanInferenceInput,
};
mod llm;
pub(super) use llm::*;
mod apply;
pub(super) use apply::*;
pub(super) fn apply_safe_augmentation() {}
const RESPONSE_KEYS: &[&str] = &["plan_field_inference", "persisted_apply"];
`);
  writeFixture(root, DEFAULT_FILES.planFieldInferenceMode, `
pub(crate) enum InferPlanFieldsMode {}
pub(crate) fn parse_infer_plan_fields_mode() {}
pub(in crate::handlers::knowledge::plan) const WORKSTATION_INFER_MODE_SONNET_SUGGEST: &str = "sonnet_suggest";
pub(in crate::handlers::knowledge::plan) enum WorkstationInferenceMode {}
pub(in crate::handlers::knowledge::plan) fn parse_workstation_inference_mode() {}
pub(in crate::handlers::knowledge::plan) fn refuse_workstation_inference_in_dag_mode() {
  scheduler_mode;
  dag_v1;
}
`);
  writeFixture(root, DEFAULT_FILES.planFieldInferenceEvidence, `
pub(super) fn scan_evidence_string_field() {}
pub(super) fn scan_evidence_string_counts() {
  typed_evidence;
}
pub(super) fn scan_evidence_string_list() {}
fn pluck_string() {}
fn pluck_string_list() {}
`);
  writeFixture(root, DEFAULT_FILES.planFieldInferenceRules, `
pub(in crate::handlers::knowledge::plan) struct PlanInferenceInput {}
pub(in crate::handlers::knowledge::plan) fn compute_plan_field_inference() {
  infer_target();
  infer_dispatch_strategy();
  infer_workstation_dispatch();
}
pub(in crate::handlers::knowledge::plan) fn caller_str() {}
pub(in crate::handlers::knowledge::plan) fn caller_bool() {}
pub(in crate::handlers::knowledge::plan) fn caller_string_list() {}
pub(super) fn infer_target() {}
pub(super) fn infer_dispatch_strategy() {}
pub(super) fn infer_workstation_dispatch() {}
pub(super) fn finalize_string_field() {}
pub(super) fn finalize_bool_field() {}
`);
  writeFixture(root, DEFAULT_FILES.planFieldInferenceLlm, `
pub(super) const LLM_ALLOWED_FIELDS: &[&str] = &["target"];
pub(super) struct LlmProposal {}
pub(super) struct LlmProposalBundle {}
pub(super) fn parse_llm_proposals() {}
pub(super) fn reconcile_llm_conflicts() {}
pub(super) fn build_llm_inference_prompt() {}
pub(super) async fn request_llm_proposals() {}
fn uses_v3_model() { load_sonnet_compiler_model(); }
pub(super) fn deterministic_covers_all_fields() {}
pub(super) async fn read_recent_evidence_entries() {}
`);
  writeFixture(root, DEFAULT_FILES.planFieldInferenceApply, `
fn apply_safe_augmentation() {}
enum ApplyOrigin {}
struct ApplyGateOutcome {}
fn validate_apply_gate_args() {}
fn compute_apply_gate() {}
mod persisted;
pub(in crate::handlers::knowledge::plan) use persisted::*;
`);
  writeFixture(root, DEFAULT_FILES.planFieldInferenceApplyPersisted, `
use super::*;
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
  existing_plan_evidence_sidecar_path();
}`);
  writeFixture(root, DEFAULT_FILES.planRouterPolicyAdapter, `
mod descriptor;
mod predicate;
mod readiness;
mod schema_parser;
use descriptor::attach_router_dispatch_descriptor;
use predicate::{arg_string, evaluate_clause, project_context};
fn parse_backend_registry(input: &str) { schema_parser::parse_backend_registry(input); }
pub(super) fn parse_router_policy(input: &str) { schema_parser::parse_router_policy(input); }
pub(super) enum RouterPolicyMode { Off, DryRun }
pub(super) fn parse_router_policy_mode() {}
pub(super) fn attach_router_recommendation_block() {}
fn compute_recommendation_block() {
  "load_trace_index";
  "load_backend_registry";
}`);
  writeFixture(root, DEFAULT_FILES.planRouterPolicyPredicate, `
pub(super) struct PredicateContext;
pub(super) fn project_context() {}
pub(super) fn arg_string() {}
pub(super) fn evaluate_clause() {
  "path-glob no match";
}
struct GlobRegex;
fn glob_match() {}
"owned_files";
`);
  writeFixture(root, DEFAULT_FILES.planRouterPolicyReadiness, `
pub(super) const RICH_TRACE_THRESHOLD: u64 = 5;
pub(super) enum TraceIndexInfo {}
pub(super) enum BackendRegistryInfo {}
pub(super) struct BackendEntry;
pub(super) fn load_trace_index() {}
pub(super) fn load_backend_registry() {}
pub(super) fn attach_backend_readiness_fields() {
  "router_apply_eligible";
  "backend_readiness_status";
}
pub(super) fn attach_trace_index_fields() {}
pub(super) fn computed_block() {}
`);
  writeFixture(root, DEFAULT_FILES.planRouterPolicyDescriptor, `
pub(super) fn attach_router_dispatch_descriptor() {
  "missiond.router-dispatch-descriptor.v1";
  "descriptor_status";
  "registry_missing";
  "source_backend_registry_path";
  "router_dispatch_descriptor";
  Value::Bool(true);
  Value::Bool(false);
}
`);
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
mod manifest;
mod projection;
use manifest::load_runner_inputs;
use projection::build_runner_response_block;
pub(super) enum TaskRunnerMode { Off, DryRun }
pub(super) fn parse_task_runner_mode() {}
pub(super) fn attach_task_runner_block() {}
pub(super) fn compute_runner_block() {}
`);
  writeFixture(root, DEFAULT_FILES.planTaskRunnerManifest, `
pub(super) fn load_runner_inputs() {}
pub(super) enum ManifestStatus {}
pub(super) struct RunnerInputs;
pub(super) struct Manifest;
pub(super) struct ManifestNode;
fn parse_manifest() {
  "MANIFEST_SCHEMA";
}
fn parse_node_entry() {}
enum Sexp {}
enum Token {}
struct TokenCursor;
`);
  writeFixture(root, DEFAULT_FILES.planTaskRunnerProjection, `
pub(super) fn build_runner_response_block() {
  Value::Bool(false);
  "manifest_status";
  "overlap_diagnostics";
  "critical_path_minutes";
  "verification_tier_counts";
}
fn project_manifest() {}
struct ManifestProjection;
struct VerificationTierCounts;
fn collect_overlap_diagnostics() {}
`);
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
use parser::{build_validated_dag, ReviewGateKind};
mod resume;
pub(super) use resume::action_execute_resume;
#[cfg(test)]
pub(super) use resume::validate_resume_request;
pub(crate) use resume::{handle_review_resolved_plan_node_event, PlanNodeResumeListenerOutcome};
mod outcome;
use outcome::{ExecutionOutcome, NodeResult, NodeState};
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
    build_finalization_block, maybe_run_distill_trigger, validate_finalize_args,
};
mod lifecycle;
use lifecycle::{
    emit_evidence_acceptance, emit_evidence_claim_conflict, emit_evidence_claim_released,
    emit_evidence_claimed, emit_evidence_dag_finalized, emit_evidence_finished,
    emit_evidence_rollback, emit_evidence_running, emit_evidence_skipped,
    emit_paused_review_gate, plan_node_should_retry, publish_plan_node_state_change, EvidenceCtx,
};
mod runtime;
use runtime::execute_with_concurrency;
#[cfg(test)]
mod tests;`);
  writeFixture(root, DEFAULT_FILES.planDagRuntime, `
mod acceptance;
mod bookkeeping;
mod claiming;
mod claims;
mod drain;
mod failures;
mod gates;
mod rollbacks;
mod retry;
mod skips;
mod spawn;
mod success;
build_node_map(&parsed.nodes);
compute_ready_ids(order, &lifecycle, &by_id);
  pub(super) async fn execute_with_concurrency() {
  tokio::task::JoinSet<Result<DispatchOutcome>>;
  ClaimRegistry::new();
  prepare_dispatch_claim();
  spawn_dispatch_attempt();
  drain_dispatch_wave();
  run_rollback(state, plan, &node).await;
  run_cascade_rollback(state, plan, &node, &parsed.nodes, order).await;
  evaluate_and_emit_rollback();
  materialize_tainted_pending_skips();
  force_skip_fail_fast_pending();
  filter_ready_nodes_for_gates();
  outcome.results = stitch_results_topologically(results_by_id, &topo_index);
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeAcceptance, `
pub(super) struct SuccessAcceptanceOutcome {
  acceptance: AcceptanceEvaluation,
  acceptance_active: bool,
  next_lifecycle: NodeLifecycle,
  next_node_state: NodeState,
  terminal_state_label: &'static str,
}
pub(super) async fn evaluate_success_acceptance() {
  evaluate_node_acceptance(node, inner_payload, true);
  apply_acceptance_fan_in();
  emit_evidence_acceptance();
  derive_acceptance_pause_id();
  AcceptanceStatus::Rejected;
  NodeState::Paused;
  NodeLifecycle::Paused;
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeBookkeeping, `
pub(super) fn build_node_map() {}
pub(super) fn build_successor_map() {}
pub(super) fn compute_ready_ids() {
  NodeLifecycle::Succeeded;
}
pub(super) fn stitch_results_topologically() {
  NodeState::SkippedUpstreamFailed;
}
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeClaiming, `
pub(super) enum DispatchClaimDecision {
  Dispatch,
  ConflictFailed { fail_fast_abort: bool },
}
pub(super) async fn prepare_dispatch_claim() {
  derive_node_claim_scopes();
  derive_plan_dag_claim_id();
  ClaimAcquire::Acquired;
  record_acquired_claim();
  ClaimAcquire::Conflict;
  emit_evidence_claim_conflict();
  NodeState::Failed;
  record_compat_claim();
  propagate_taint();
  FAILURE_POLICY_FAIL_FAST;
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeClaims, `
pub(super) async fn record_acquired_claim() {
  emit_evidence_claimed();
  NodeLifecycle::Claimed;
  active_claims_by_node.insert(node.id.clone(), claim.claim_id.clone());
}
pub(super) async fn record_compat_claim() {
  emit_evidence_claimed();
  NodeLifecycle::Claimed;
}
pub(super) async fn release_claim_if_recorded() {
  active_claims_by_node.remove(&node.id);
  claim_registry.release(&claim_id, chrono::Utc::now());
  emit_evidence_claim_released();
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeDrain, `
pub(super) async fn drain_dispatch_wave() {
  join_set.join_next().await;
  emit_evidence_finished();
  record_successful_dispatch();
  retry_failed_node_if_allowed();
  record_final_failure();
  DispatchOutcome;
  classification.is_ok();
  abort_aborter = Some(node_id.clone());
  DAG scheduler: dispatch task join failed;
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeFailures, `
pub(super) async fn record_final_failure() {
  NodeLifecycle::Failed;
  release_claim_if_recorded();
  NodeState::Failed;
  evaluate_and_emit_rollback();
  retry_skipped_non_retryable: non_retryable;
  propagate_taint();
  FAILURE_POLICY_FAIL_FAST;
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeGates, `
pub(super) async fn filter_ready_nodes_for_gates() {
  ReviewGateKind::QuestionEvent;
  emit_paused_review_gate();
  emit_evidence_skipped();
  NodeState::SkippedCondition;
  NodeState::Paused;
  propagate_taint();
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeRollbacks, `
pub(super) async fn evaluate_and_emit_rollback() {
  run_rollback();
  run_cascade_rollback();
  emit_evidence_rollback();
  node.has_active_rollback_cascade();
  evaluation.cascade = Some(cascade);
  evaluation.is_inactive();
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeRetry, `
pub(super) async fn retry_failed_node_if_allowed() {
  plan_node_should_retry();
  release_claim_if_recorded();
  node.effective_retry_delay_ms();
  derive_plan_dag_claim_id();
  ClaimAcquire::Acquired;
  record_acquired_claim();
  ClaimAcquire::Conflict;
  record_compat_claim();
  spawn_dispatch_attempt();
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeSkips, `
pub(super) async fn materialize_tainted_pending_skips() {
  collect_tainted_pending();
  NodeState::SkippedUpstreamFailed;
  emit_evidence_skipped();
}
pub(super) async fn force_skip_fail_fast_pending() {
  pending_ids();
  NodeState::SkippedFailFastAbort;
  emit_evidence_skipped();
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeSpawn, `
pub(super) async fn spawn_dispatch_attempt() {
  NodeLifecycle::Running;
  emit_evidence_running();
  dispatch_node();
  join_set.spawn();
  task_contract_ctx.clone();
}`);
  writeFixture(root, DEFAULT_FILES.planDagRuntimeSuccess, `
pub(super) async fn record_successful_dispatch() {
  evaluate_success_acceptance();
  release_claim_if_recorded();
  evaluate_and_emit_rollback();
  propagate_taint();
  NodeResult;
  retry_skipped_non_retryable: false;
  FAILURE_POLICY_FAIL_FAST;
}`);
  writeFixture(root, DEFAULT_FILES.planDagOutcome, `
mod execution;
mod node_result;
mod state;
pub(super) use execution::ExecutionOutcome;
pub(super) use node_result::NodeResult;
pub(super) use state::{NodeLifecycle, NodeState};
`);
  writeFixture(root, DEFAULT_FILES.planDagOutcomeState, `
pub(in crate::handlers::knowledge::plan_dag) enum NodeState {
  Succeeded,
  SkippedUpstreamFailed,
  SkippedCondition,
  SkippedFailFastAbort,
  Paused,
}
pub(in crate::handlers::knowledge::plan_dag) enum NodeLifecycle {}
`);
  writeFixture(root, DEFAULT_FILES.planDagOutcomeNodeResult, `
use serde_json::Value;
use super::super::acceptance::AcceptanceEvaluation;
use super::super::rollback::RollbackEvaluation;
pub(in crate::handlers::knowledge::plan_dag) struct NodeResult {
  attempts_made: u32,
  retry_skipped_non_retryable: bool,
}
impl NodeResult {
  pub(in crate::handlers::knowledge::plan_dag) fn skipped() {
    Value::Null;
  }
}
impl Default for NodeResult {}
`);
  writeFixture(root, DEFAULT_FILES.planDagOutcomeExecution, `
use missiond_core::types::PlanStatus;
pub(in crate::handlers::knowledge::plan_dag) struct ExecutionOutcome {
  retry_skipped_non_retryable: bool,
}
impl ExecutionOutcome {
  pub(in crate::handlers::knowledge::plan_dag) fn node_results_json() {
    review_question_warning;
  }
  pub(in crate::handlers::knowledge::plan_dag) fn paused_nodes_json() {}
  pub(in crate::handlers::knowledge::plan_dag) fn skipped_nodes_json() {}
  pub(in crate::handlers::knowledge::plan_dag) fn aggregate_status() {}
  pub(in crate::handlers::knowledge::plan_dag) fn runner_status() {}
  pub(in crate::handlers::knowledge::plan_dag) fn target_plan_status() -> Option<PlanStatus> {
    PlanStatus::Succeeded;
    PlanStatus::Failed;
    None
  }
}
`);
  writeFixture(root, DEFAULT_FILES.planDagDispatch, `
mod runner;
mod task_contract_ctx;
mod types;
mod workstation;
pub(super) use runner::dispatch_node;
pub(super) use task_contract_ctx::TaskContractDispatchCtx;
pub(super) use types::DispatchOutcome;
`);
  writeFixture(root, DEFAULT_FILES.planDagDispatchTypes, `
pub(in crate::handlers::knowledge::plan_dag) struct DispatchOutcome {
  node_id: String,
  inner_payload: Value,
  classification: std::result::Result<(), String>,
  non_retryable: bool,
}
`);
  writeFixture(root, DEFAULT_FILES.planDagDispatchWorkstation, `
pub(in crate::handlers::knowledge::plan_dag) fn node_to_workstation_hints() {
  workstation_dispatch::WorkstationDispatchHints;
  plan::split_lisp_string_list(raw);
}
pub(in crate::handlers::knowledge::plan_dag) fn workstation_outcome_to_dispatch_pair() {
  workstation_dispatch::outcome_to_response_fields(outcome, strategy);
  WorkstationDispatchOutcome::SafeDescriptor;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagDispatchTaskContractCtx, `
pub(in crate::handlers::knowledge::plan_dag) struct TaskContractDispatchCtx {
  pub mode: plan::TaskContractEmitMode,
  pub dispatch_contract_mode: plan::DispatchContractMode,
}
impl TaskContractDispatchCtx {
  fn off() {}
  fn from_args() {
    plan::parse_task_contract_emit_mode(args);
    plan::parse_dispatch_contract_mode(args);
  }
}
`);
  writeFixture(root, DEFAULT_FILES.planDagDispatchRunner, `
pub(in crate::handlers::knowledge::plan_dag) async fn dispatch_node() {
  workstation_dispatch::run_workstation_dispatch_with_contract();
  agent_execution::handle();
  task_delegate::handle();
  flow_run::handle();
  plan::emit_task_contract();
  plan::merge_task_contract_block();
  tool_result_payload();
  build_node_inner_args();
  node_to_workstation_hints();
  workstation_outcome_to_dispatch_pair();
}
`);
  writeFixture(root, DEFAULT_FILES.planDagParser, `
mod scanner;
mod types;
mod validation;
pub(super) use scanner::parse_plan_dag;
pub(super) use validation::build_validated_dag;
pub(in crate::handlers::knowledge) use types::{DagNode, ParsedDag};
`);
  writeFixture(root, DEFAULT_FILES.planDagParserTypes, `
mod errors;
mod node;
pub(in crate::handlers::knowledge::plan_dag) use errors::DagBuildError;
pub(in crate::handlers::knowledge) use node::{DagNode, ParsedDag};
pub(in crate::handlers::knowledge::plan_dag) use node::{
  ReviewGateKind, FAILURE_POLICY_FAIL_FAST, MAX_NODE_ATTEMPTS_CAP, MAX_RETRY_DELAY_MS,
};
pub(super) use node::{FAILURE_POLICY_CONTINUE, VALID_TARGETS};
`);
  writeFixture(root, DEFAULT_FILES.planDagParserTypesNode, `
pub(in crate::handlers::knowledge::plan_dag::parser) const VALID_TARGETS: &[&str] = &[];
pub(in crate::handlers::knowledge::plan_dag) const MAX_NODE_ATTEMPTS_CAP: u32 = 3;
pub(in crate::handlers::knowledge::plan_dag) const MAX_RETRY_DELAY_MS: u64 = 60000;
pub(in crate::handlers::knowledge) struct DagNode;
impl DagNode {
  pub(in crate::handlers::knowledge::plan_dag) fn acceptance_mode_kind() {
    AcceptanceRequires::parse();
  }
  pub(in crate::handlers::knowledge::plan_dag) fn has_acceptance_fan_in() {}
}
pub(in crate::handlers::knowledge::plan_dag) enum ReviewGateKind { None }
pub(in crate::handlers::knowledge) struct ParsedDag;
`);
  writeFixture(root, DEFAULT_FILES.planDagParserTypesErrors, `
pub(in crate::handlers::knowledge::plan_dag) enum DagBuildError { NoNodes, InvalidTarget, InvalidRetryHint, AcceptanceFanInRequiresMissing, CompensateDirectionMismatch }
impl DagBuildError {
  pub(in crate::handlers::knowledge::plan_dag) fn into_tool_result() {
    error_codes::INVALID_PARAM;
    DagBuildError::NoNodes;
    DagBuildError::InvalidTarget;
    DagBuildError::InvalidRetryHint;
    DagBuildError::AcceptanceFanInRequiresMissing;
    DagBuildError::CompensateDirectionMismatch;
    VALID_TARGETS;
  }
}
`);
  writeFixture(root, DEFAULT_FILES.planDagParserScanner, `
mod keyword_pairs;
mod lists;
mod node_form;
mod top_level;
pub(in crate::handlers::knowledge::plan_dag) use top_level::parse_plan_dag;
`);
  writeFixture(root, DEFAULT_FILES.planDagParserScannerTopLevel, `
pub(in crate::handlers::knowledge::plan_dag) fn parse_plan_dag() {
  parse_node_form(&form);
  unsupported_top_forms.push(form);
}
pub(super) fn scan_top_level_forms() {}
pub(super) fn top_form_head() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagParserScannerNodeForm, `
pub(super) fn parse_node_form() {
  scan_keyword_pairs(form);
  parse_id_list(&value);
  AcceptanceMode::parse(raw);
  AcceptanceRequires::parse(raw);
  RollbackPolicy::parse(raw);
  RollbackCascadeMode::parse();
  unsupported_fields.push((raw_key.clone(), value.clone()));
  match key.as_str() {
    "target" | "target-tool" | "tool" => {}
    "objective" => set_first(&mut objective, &value)
    "timeout-ms" | "timeout_ms" => {}
    "target-project" | "target_project" | "project" => {}
    "requested-cwd" | "requested_cwd" | "cwd" => {}
    "acceptance-commands" | "acceptance_commands" => {}
    "workstation-dispatch" | "workstation_dispatch" => {}
  }
  DagNode {};
}
`);
  writeFixture(root, DEFAULT_FILES.planDagParserScannerLists, `
pub(super) fn parse_id_list() {
  strip_prefix('[');
  let mut esc = false;
  out.push(s);
}
`);
  writeFixture(root, DEFAULT_FILES.planDagParserScannerKeywordPairs, `
pub(super) fn scan_keyword_pairs() {
  let mut in_string = false;
  let key: String = String::new();
  out.push((key, value));
}
`);
  writeFixture(root, DEFAULT_FILES.planDagParserValidation, `
pub(in crate::handlers::knowledge::plan_dag) fn build_validated_dag() {
  AcceptanceRequires::EvidenceKeys;
  VALID_TARGETS.contains(&target);
  compute_transitive_ancestors();
}
fn compute_transitive_ancestors() {}
fn kahn_topo_sort() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagAcceptance, `
mod evaluator;
mod fan_in;
mod payload;
mod pause;
mod types;
pub(super) use evaluator::evaluate_node_acceptance;
pub(super) use fan_in::apply_acceptance_fan_in;
pub(super) use pause::derive_acceptance_pause_id;
pub(super) use types::{
};
`);
  writeFixture(root, DEFAULT_FILES.planDagAcceptanceTypes, `
pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceMode { InnerStatus }
impl AcceptanceMode {
  pub(in crate::handlers::knowledge::plan_dag) fn as_wire() {}
  pub(in crate::handlers::knowledge::plan_dag) fn parse() {}
}
pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceRequires { AllSucceeded }
impl AcceptanceRequires {
  pub(in crate::handlers::knowledge::plan_dag) fn as_wire() {}
  pub(in crate::handlers::knowledge::plan_dag) fn parse() {}
}
pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceStatus { Accepted }
impl AcceptanceStatus { pub(in crate::handlers::knowledge::plan_dag) fn as_wire() {} }
pub(in crate::handlers::knowledge::plan_dag) struct AcceptanceEvaluation;
impl AcceptanceEvaluation {
  pub(in crate::handlers::knowledge::plan_dag) fn is_inactive() {}
  pub(in crate::handlers::knowledge::plan_dag) fn to_json() {}
}
pub(in crate::handlers::knowledge::plan_dag) struct AcceptanceFanInOutcome;
impl AcceptanceFanInOutcome {
  pub(in crate::handlers::knowledge::plan_dag) fn to_json() {}
}
`);
  writeFixture(root, DEFAULT_FILES.planDagAcceptanceEvaluator, `
pub(in crate::handlers::knowledge::plan_dag) fn evaluate_node_acceptance() {
  AcceptanceMode::parse(raw);
  AcceptanceStatus::ManualRequired;
  AcceptanceStatus::Rejected;
  inner_payload_failure_signal(payload);
  inner_payload_missing_keys(payload, keys);
  split_lisp_string_list(raw);
}
`);
  writeFixture(root, DEFAULT_FILES.planDagAcceptanceFanIn, `
pub(in crate::handlers::knowledge::plan_dag) fn apply_acceptance_fan_in() {
  AcceptanceRequires::AllSucceeded;
  AcceptanceRequires::AnySucceeded;
  AcceptanceRequires::EvidenceKeys;
  inner_payload_missing_keys(payload, keys);
  AcceptanceFanInOutcome {};
}
`);
  writeFixture(root, DEFAULT_FILES.planDagAcceptancePayload, `
pub(super) fn inner_payload_failure_signal() {
  workstation_dispatch_status;
}
pub(super) fn inner_payload_missing_keys() {
  typed_evidence;
  inner_payload_contains_key();
}
fn inner_payload_contains_key() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagAcceptancePause, `
pub(in crate::handlers::knowledge::plan_dag) fn derive_acceptance_pause_id() {
  "acceptance:plan:{}:v{}:{}";
}
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
mod cascade;
mod descriptor;
mod run;
mod types;
pub(super) use cascade::{compute_compensation_order, run_cascade_rollback};
pub(super) use descriptor::{build_rollback_descriptor, pre_dispatch_rollback_decision, RollbackDescriptor};
pub(super) use run::{run_rollback, truncate_rollback_brief_preview};
pub(super) use types::{CascadeCompensationOutcome, CascadeRollbackOutcome, RollbackCascadeMode, RollbackEvaluation, RollbackPolicy, RollbackStatus};
use super::DagNode;
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackTypes, `
mod cascade;
mod evaluation;
mod node_ext;
mod policy;
pub(in crate::handlers::knowledge::plan_dag) use cascade::{CascadeCompensationOutcome, CascadeRollbackOutcome, RollbackCascadeMode};
pub(in crate::handlers::knowledge::plan_dag) use evaluation::{RollbackEvaluation, RollbackStatus};
pub(in crate::handlers::knowledge::plan_dag) use policy::RollbackPolicy;
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackTypesNodeExt, `
impl DagNode {
  fn rollback_policy_kind(&self) { RollbackPolicy::parse("none"); }
  fn rollback_cascade_kind(&self) { RollbackCascadeMode::parse("none"); }
  fn has_active_rollback_cascade(&self) {}
  fn has_rollback_hints(&self) {}
}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackTypesPolicy, `
pub(in crate::handlers::knowledge::plan_dag) enum RollbackPolicy { None, Descriptor, Workstation }
impl RollbackPolicy {
  fn as_wire(&self) {
    RollbackPolicy::None;
    RollbackPolicy::Descriptor;
    RollbackPolicy::Workstation;
  }
  fn parse(raw: &str) {
    RollbackPolicy::None;
    RollbackPolicy::Descriptor;
    RollbackPolicy::Workstation;
  }
}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackTypesEvaluation, `
pub(in crate::handlers::knowledge::plan_dag) enum RollbackStatus { NotRequested, DescriptorReady, Dispatched, Refused, Failed }
pub(in crate::handlers::knowledge::plan_dag) struct RollbackEvaluation;
impl RollbackEvaluation {
  fn is_inactive(&self) {}
  fn to_json(&self) {
    RollbackStatus::NotRequested;
    RollbackStatus::DescriptorReady;
    RollbackStatus::Dispatched;
    RollbackStatus::Refused;
    RollbackStatus::Failed;
    cascade.to_json();
  }
}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackTypesCascade, `
pub(in crate::handlers::knowledge::plan_dag) enum RollbackCascadeMode { None, PlanOnly, DispatchSafe }
pub(in crate::handlers::knowledge::plan_dag) struct CascadeCompensationOutcome;
pub(in crate::handlers::knowledge::plan_dag) struct CascadeRollbackOutcome;
fn uses_status(_: RollbackStatus) {}
impl CascadeCompensationOutcome {
  fn is_inactive(&self) {}
  fn to_json(&self) {
    RollbackCascadeMode::DispatchSafe;
    let _ = "compensations";
  }
}
impl CascadeRollbackOutcome {
  fn is_inactive(&self) {}
  fn to_json(&self) {
    RollbackCascadeMode::DispatchSafe;
    let _ = "compensations";
  }
}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackDescriptor, `
pub(in crate::handlers::knowledge::plan_dag) fn build_rollback_descriptor() {}
pub(in crate::handlers::knowledge::plan_dag) struct RollbackDescriptor;
impl RollbackDescriptor {
  pub(in crate::handlers::knowledge::plan_dag) fn to_workstation_hints() {}
  pub(in crate::handlers::knowledge::plan_dag) fn safety_check_for_workstation() {
    "rollback workstation dispatch requires :rollback-objective";
    workstation_dispatch::INFERABLE_DISPATCH_STRATEGIES;
  }
}
pub(in crate::handlers::knowledge::plan_dag) fn pre_dispatch_rollback_decision() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackRun, `
pub(in crate::handlers::knowledge::plan_dag) async fn run_rollback() {
  workstation_dispatch::run_workstation_dispatch;
  WorkstationDispatchOutcome::Dispatched;
  WorkstationDispatchOutcome::DryRun;
  WorkstationDispatchOutcome::InnerError;
  WorkstationDispatchOutcome::SafeDescriptor;
}
pub(in crate::handlers::knowledge::plan_dag) fn truncate_rollback_brief_preview() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackCascade, `
mod dispatch_outcome;
mod ordering;
mod plan_entry;
mod runner;
pub(in crate::handlers::knowledge::plan_dag) use ordering::compute_compensation_order;
pub(in crate::handlers::knowledge::plan_dag) use plan_entry::build_compensation_plan_entry;
pub(in crate::handlers::knowledge::plan_dag) use runner::run_cascade_rollback;
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackCascadeOrdering, `
use std::collections::{HashMap, HashSet};
pub(in crate::handlers::knowledge::plan_dag) fn compute_compensation_order() {}
fn details() {
  rollback_after;
  compensate_node;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackCascadePlanEntry, `
pub(in crate::handlers::knowledge::plan_dag) fn build_compensation_plan_entry() {}
fn details() {
  build_rollback_descriptor(node);
  descriptor.to_workstation_hints(node);
  workstation_dispatch::build_task_brief(plan, hints, strategy);
  RollbackStatus::DescriptorReady;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackCascadeRunner, `
pub(in crate::handlers::knowledge::plan_dag) async fn run_cascade_rollback() {}
fn details() {
  compute_compensation_order();
  build_compensation_plan_entry();
  map_dispatch_outcome_to_compensation();
  RollbackCascadeMode::DispatchSafe;
  workstation_dispatch::run_workstation_dispatch();
}
`);
  writeFixture(root, DEFAULT_FILES.planDagRollbackCascadeDispatchOutcome, `
pub(super) fn map_dispatch_outcome_to_compensation() {
  WorkstationDispatchOutcome;
  O::Dispatched;
  O::DryRun;
  O::InnerError;
  O::SafeDescriptor;
  truncate_rollback_brief_preview();
}
`);
  writeFixture(root, DEFAULT_FILES.planDagResume, `
mod action;
mod evidence;
mod listener;
mod validation;
pub(in crate::handlers::knowledge) use action::action_execute_resume;
pub(crate) use listener::{handle_review_resolved_plan_node_event, PlanNodeResumeListenerOutcome};
pub(in crate::handlers::knowledge) use validation::{validate_resume_request, PlanNodeResumeError};
`);
  writeFixture(root, DEFAULT_FILES.planDagResumeValidation, `
use crate::handlers::knowledge::review_gate::{derive_plan_node_topic_hash, is_plan_node_review_action, ParsedReviewQuestionId};
pub(in crate::handlers::knowledge) enum PlanNodeResumeError { MissingTopicHash }
impl PlanNodeResumeError {
  pub(in crate::handlers::knowledge::plan_dag) fn code() {}
  pub(in crate::handlers::knowledge::plan_dag) fn message() {}
}
pub(in crate::handlers::knowledge) fn validate_resume_request() {
  ReviewGateKind::QuestionEvent;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagResumeAction, `
pub(in crate::handlers::knowledge) async fn action_execute_resume() {
  parse_review_question_id_struct();
  PlanNodeResumeInput;
  TaskContractDispatchCtx::from_args();
  "resume_dispatched";
  "resume_failed";
  bus_publish_warnings;
}
fn resume_error_to_tool_result() {}
`);
  writeFixture(root, DEFAULT_FILES.planDagResumeEvidence, `
async fn emit_resume_decision_evidence() {
  publish_plan_node_state_change();
  "paused ->";
  EvidenceEntry::new();
  PLAN_DAG_NODE_DISPATCH;
  review_resume;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagResumeListener, `
pub(crate) enum PlanNodeResumeListenerOutcome { NotFound }
pub(crate) async fn handle_review_resolved_plan_node_event() {
  ArtifactIdNotUuid;
  ValidationRejected;
  TaskContractDispatchCtx::off();
  PlanNodeResumeInput;
  ReviewDecision::Approved;
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
pub(super) async fn maybe_run_distill_trigger() {
  super::super::workflow::handle();
  tool_result_payload();
  distill_invoked_ok;
  distill_invoked_handler_error;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycle, `
mod claims;
mod context;
mod event_ref;
mod finalize;
mod nodes;
mod retry;
mod review;
pub(super) use claims::*;
pub(super) use context::EvidenceCtx;
pub(super) use event_ref::publish_plan_node_state_change;
pub(super) use finalize::emit_evidence_dag_finalized;
pub(super) use nodes::{};
pub(super) use retry::{};
pub(super) use review::emit_paused_review_gate;
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleContext, `
pub(in crate::handlers::knowledge::plan_dag) struct EvidenceCtx<'a> {
  plan_id: uuid::Uuid,
  plan_version: i32,
  target_project_arg: Option<&'a str>,
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleEventRef, `
pub(in crate::handlers::knowledge::plan_dag) const EVENT_REF_SOURCE_EXECUTION: &str = "execution";
pub(in crate::handlers::knowledge::plan_dag) const EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED: &str = "plan_node_state_changed";
pub(in crate::handlers::knowledge::plan_dag) fn deterministic_plan_node_event_id() {}
pub(in crate::handlers::knowledge::plan_dag) fn build_plan_node_state_changed_event() {
  ExecutionEvent::PlanNodeStateChanged;
}
pub(in crate::handlers::knowledge::plan_dag) async fn publish_plan_node_state_change() {
  EventRef::new();
  lookup_or_query_plan_node_state_change();
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleFinalize, `
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_dag_finalized() {
  with_state_transition("dag_finalized");
  with_extra("event_kind", json!("plan_dag_finalized"));
  with_extra("distill", d.clone());
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleNodes, `
mod acceptance;
mod finished;
mod rollback;
mod running;
mod skipped;
pub(in crate::handlers::knowledge::plan_dag) use acceptance::emit_evidence_acceptance;
pub(in crate::handlers::knowledge::plan_dag) use finished::emit_evidence_finished;
pub(in crate::handlers::knowledge::plan_dag) use rollback::emit_evidence_rollback;
pub(in crate::handlers::knowledge::plan_dag) use running::emit_evidence_running;
pub(in crate::handlers::knowledge::plan_dag) use skipped::emit_evidence_skipped;
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleNodesRunning, `
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_running() {
  with_state_transition("ready -> running");
  publish_plan_node_state_change();
  with_primary_event_ref();
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleNodesFinished, `
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_finished() {
  with_state_transition("running -> succeeded");
  with_state_transition("running -> failed");
  with_inner_dispatch(inner_payload.clone());
  with_extra("inner_error", inner_payload.clone());
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleNodesRollback, `
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_rollback() {
  let _ = "failed -> rollback_";
  rollback_policy;
  rollback_cascade;
  rollback_inner_result;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleNodesAcceptance, `
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_acceptance() {
  let _ = "succeeded -> acceptance_";
  acceptance_fan_in;
  acceptance_pause_id;
  derive_acceptance_pause_id();
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleNodesSkipped, `
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_skipped() {
  with_state_transition("pending -> skipped");
  skip_reason;
  PLAN_NODE_DEFAULT_ATTEMPT;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleRetry, `
pub(in crate::handlers::knowledge::plan_dag) const PLAN_NODE_DEFAULT_ATTEMPT: u32 = 1;
pub(in crate::handlers::knowledge::plan_dag) fn plan_node_should_retry() {
  saturating_sub(current_attempt);
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleReview, `
pub(in crate::handlers::knowledge::plan_dag) async fn emit_paused_review_gate() {
  publish_question(ev);
  derive_plan_node_review_question_id();
  with_state_transition("pending -> paused");
  review_question_warning;
}
`);
  writeFixture(root, DEFAULT_FILES.planDagLifecycleClaims, `
use super::{publish_plan_node_state_change, EvidenceCtx};
use super::super::claim_lease::PlanDagClaim;
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claimed() { with_state_transition("pending -> claimed"); }
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claim_released() { with_state_transition("claimed -> released"); }
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claim_conflict() { with_state_transition("pending -> failed"); let _ = "claim_conflict"; }
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
.missiond/v3/runtime/plans/<plan_id>.evidence.json
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
