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
  - unified_entry forwards plan compile/execute args instead of inventing a second plan schema.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  planHandler: 'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
  planCompileAuthoring: 'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs',
  planExecuteHints: 'crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs',
  planTaskContract: 'crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs',
  planDistillChain: 'crates/missiond-daemon/src/handlers/knowledge/plan/distill_chain.rs',
  planDispatchResponse: 'crates/missiond-daemon/src/handlers/knowledge/plan/dispatch_response.rs',
  planEvidenceSidecar: 'crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs',
  planRouterPolicyAdapter: 'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs',
  planTaskRunnerAdapter: 'crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs',
  planDag: 'crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs',
  unifiedEntry: 'crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs',
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
    'plan/execute_hints.rs owns mission_plan PLAN.lisp hint parsing',
    'plan/task_contract.rs owns mission_plan task-contract Lisp projection',
    'plan/distill_chain.rs owns mission_plan cross-plan distill-chain egress',
    'plan/dispatch_response.rs owns mission_plan execution response egress',
    'plan/evidence_sidecar.rs owns mission_plan evidence sidecar egress',
    'plan/router_policy_dry_run.rs owns the mission_plan router-policy adapter',
    'plan/task_runner_dry_run.rs owns the mission_plan task-runner adapter',
    'execute can derive target_source=plan_hint from plan.sexp_text',
    'DAG execution parses node-local Lisp hints',
    'node scripts/check-v3-plan-execution-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.planHandler, sources.planHandler, [
    'mod compile_authoring',
    'use compile_authoring::{action_compile, collect_string_list}',
    'parse_plan_hints(&plan.sexp_text)',
    '(t, "plan_hint")',
    'target_source',
    'dispatch_strategy_source',
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
    'pub(super) enum RouterPolicyMode',
    'pub(super) fn parse_router_policy_mode',
    'pub(super) fn attach_router_recommendation_block',
    'fn compute_recommendation_block',
    'router_apply_eligible',
    'router_dispatch_descriptor',
    'backend_readiness_status',
    'Value::Bool(false)',
  ]);

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
    'fn parse_node_form',
    '"target" | "target-tool" | "tool"',
    '"objective" => set_first(&mut objective, &value)',
    '"timeout-ms" | "timeout_ms"',
    '"target-project" | "target_project" | "project"',
    '"requested-cwd" | "requested_cwd" | "cwd"',
    '"acceptance-commands" | "acceptance_commands"',
    '"workstation-dispatch" | "workstation_dispatch"',
    'node_args.insert("timeout_secs".to_string()',
    'build_internal_dispatch_args(',
    'run_workstation_dispatch',
  ]);

  requireAll(diagnostics, files.unifiedEntry, sources.unifiedEntry, [
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
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/distill_chain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/dispatch_response.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs"]
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold; plan/compile_authoring.rs owns mission_plan plan-authoring entry/core; plan/execute_hints.rs owns mission_plan PLAN.lisp hint parsing; plan/task_contract.rs owns mission_plan task-contract Lisp projection; plan/distill_chain.rs owns mission_plan cross-plan distill-chain egress; plan/dispatch_response.rs owns mission_plan execution response egress; plan/evidence_sidecar.rs owns mission_plan evidence sidecar egress; plan/router_policy_dry_run.rs owns the mission_plan router-policy adapter; plan/task_runner_dry_run.rs owns the mission_plan task-runner adapter; execute can derive target_source=plan_hint from plan.sexp_text. DAG execution parses node-local Lisp hints."))
  (compression-contract
    :checks ["node scripts/check-v3-plan-execution-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.planHandler, `
mod compile_authoring;
use compile_authoring::{action_compile, collect_string_list};
parse_plan_hints(&plan.sexp_text);
let x = (t, "plan_hint");
let target_source = "";
let dispatch_strategy_source = "";`);
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
pub(super) enum RouterPolicyMode { Off, DryRun }
pub(super) fn parse_router_policy_mode() {}
pub(super) fn attach_router_recommendation_block() {}
fn compute_recommendation_block() {
  "router_apply_eligible";
  "router_dispatch_descriptor";
  "backend_readiness_status";
  Value::Bool(false);
}`);
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
run_workstation_dispatch();`);
  writeFixture(root, DEFAULT_FILES.unifiedEntry, `
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
