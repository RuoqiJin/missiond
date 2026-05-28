#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-control-plane-kernel-isomorphism.mjs [--json]

Checks the hard-cut control-plane kernel contract:
  - V3 declares typed runtime facts, runtime ABI fields, and hard-cut rules.
  - Postgres has capability/job/lease/projection tables and constraints.
  - Completion, claim, delegation, sandbox, and frontend error paths use typed facts/codes.
  - Non-core full-os tools stay behind explicit feature gates and return FEATURE_DISABLED.
`;

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  migration: 'crates/missiond-core/migrations/20260527000000_control_plane_kernel.sql',
  controlPlaneKernel: 'crates/missiond-daemon/src/engine/control_plane_kernel.rs',
  sharedMemory: 'crates/missiond-daemon/src/engine/shared_memory.rs',
  evidenceWriter: 'crates/missiond-daemon/src/engine/task_completion_evidence.rs',
  featureGates: 'crates/missiond-daemon/src/feature_gates.rs',
  handlers: 'crates/missiond-daemon/src/handlers/mod.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  deployDaemon: 'scripts/deploy-daemon.sh',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  flowEngine: 'crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs',
  taskDelegate: 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
  agentExecutionClaimLease: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs',
  agentExecutionClaimRelease: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_release.rs',
  agentExecutionClaimHeartbeat: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_heartbeat.rs',
  v2Subscribers: 'crates/missiond-daemon/src/bus/v2_subscribers.rs',
  computeSlot: 'crates/missiond-daemon/src/handlers/compute/compute_slot.rs',
  ptyHandler: 'crates/missiond-daemon/src/handlers/compute/pty.rs',
  spawner: 'crates/missiond-daemon/src/slot_orchestrator/spawner.rs',
  boardStore: 'crates/missiond-core/src/db/pg/board.rs',
  boardTypes: 'crates/missiond-core/src/types/board.rs',
  dbError: 'crates/missiond-core/src/db/error.rs',
  mcpTools: 'crates/missiond-mcp/src/tools/mod.rs',
  mcpGateway: 'crates/missiond-mcp/src/gen_gateway.rs',
  boardHandler: 'crates/missiond-daemon/src/handlers/knowledge/board.rs',
  boardCreateHandler: 'crates/missiond-daemon/src/handlers/knowledge/board/create.rs',
  sharedHandler: 'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs',
  boardRoute: 'packages/board/src/app/api/tasks/route.ts',
  boardStoreTs: 'packages/board/src/store.ts',
  verifierRouterMigration: 'crates/missiond-core/migrations/20260527001000_runtime_verifier_router_outcomes.sql',
  capabilityGrantOperationMigration: 'crates/missiond-core/migrations/20260527002000_capability_grants_spawn_operation.sql',
  kernelReverseMigration: 'crates/missiond-core/migrations/20260527003000_kernel_reverse_convergence.sql',
  backfillRuntimeMetadata: 'scripts/backfill-board-runtime-metadata.mjs',
};

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = checkFiles(process.cwd(), FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(FILES).length,
    diagnostics,
  };

  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 control-plane kernel Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 control-plane kernel Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }

  process.exit(result.ok ? 0 : 1);
}

function parseArgs(args) {
  const opts = { json: false };
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    }
    if (arg === '--json') {
      opts.json = true;
      continue;
    }
    console.error(`unknown arg: ${arg}`);
    console.error(usage);
    process.exit(2);
  }
  return opts;
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = key === 'blueprint'
        ? readBlueprintWithEvidenceSidecars(root, rel)
        : fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    '(control-plane-kernel',
    ':schema "missiond.control-plane-kernel.v1"',
    ':facts [task_contracts task_result_artifacts event_log jobs job_attempts work_leases capability_grants capability_audit_events review_gates board_task_views]',
    ':runtime-abi-fields [completion_artifact_schema job_state_machine capability_policy sandbox_policy projection_policy]',
    ':kernel-core [delegate claim-lease capability spawn attempt completion-artifact settle event-log board-projection pty-worker-adapter]',
    ':optional-full-os-layers [workflow memory skill-store router-experiments codex-replay self-evolution advanced-conversations infra-os advanced-board]',
    ':feature-gates [MISSIOND_FULL_OS_ENABLE',
    ':hard-cutover true',
    'BoardTask description, Board notes, PTY screens, TUI summaries, and provider prose are projection/observation inputs only.',
    'Runtime control paths MUST read task_contracts, subject-bound capability_grants, work_leases, jobs/job_attempts, event_log, and task_result_artifacts',
    'Kernel-internal completion writes MUST call typed task_result_put entrypoints directly',
    'Missing task_contracts on a control-plane task returns TASK_CONTRACT_REQUIRED',
    'task_result_put and worker_settle MUST pass exact grant_id + subject_kind + subject_id + operation + scope + task_id capability checks',
    'Write-scoped completion verification MUST bind to the current job_attempt',
    'compares git status plus git diff --name-only between pre/post HEAD',
    'Worker spawn MUST carry an exact subject-bound worker/conversation spawn grant',
    'BoardTask claim, release, heartbeat, expiry, and recovery MUST use work_leases as the lease authority',
    'ProjectionEngine updates board_task_views and Board-facing status from typed events/state',
    'Non-core full-os tools MUST keep their public MCP names but default to FEATURE_DISABLED',
    'Startup services for self-evolution, Lisp code sync, workflow recovery, memory embeddings, and multi-provider diagnostics MUST NOT start in kernel-core mode.',
    'Blue-green launchd deployment MUST propagate MISSIOND_FULL_OS_ENABLE and individual MISSIOND_FEATURE_* gates',
    ':checker "scripts/check-v3-control-plane-kernel-isomorphism.mjs"',
    'node scripts/check-v3-control-plane-kernel-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.featureGates, sources.featureGates, [
    'pub(crate) const FULL_OS_ENV: &str = "MISSIOND_FULL_OS_ENABLE";',
    'pub(crate) const WORKFLOW_ENV: &str = "MISSIOND_FEATURE_WORKFLOW_ENABLE";',
    'pub(crate) const MEMORY_ENV: &str = "MISSIOND_FEATURE_MEMORY_ENABLE";',
    'pub(crate) const SKILL_STORE_ENV: &str = "MISSIOND_FEATURE_SKILL_STORE_ENABLE";',
    'pub(crate) const ROUTER_EXPERIMENTS_ENV: &str = "MISSIOND_FEATURE_ROUTER_EXPERIMENTS_ENABLE";',
    'pub(crate) const CODEX_REPLAY_ENV: &str = "MISSIOND_FEATURE_CODEX_REPLAY_ENABLE";',
    'pub(crate) const SELF_EVOLUTION_ENV: &str = "MISSIOND_FEATURE_SELF_EVOLUTION_ENABLE";',
    'pub(crate) const CONVERSATIONS_ENV: &str = "MISSIOND_FEATURE_CONVERSATIONS_ENABLE";',
    'pub(crate) const INFRA_OS_ENV: &str = "MISSIOND_FEATURE_INFRA_OS_ENABLE";',
    'pub(crate) const BOARD_ADVANCED_ENV: &str = "MISSIOND_FEATURE_BOARD_ADVANCED_ENABLE";',
    'pub(crate) fn optional_feature_for_tool',
    'error_codes::FEATURE_DISABLED',
    'disabled in kernel-core mode',
    'mission_task_delegate',
    'mission_compute_slot',
    'mission_shared_memory',
    'mission_plan',
    'mission_workflow',
    'mission_memory',
    'mission_skill_exec',
    'mission_router_chat',
    'mission_codex_replay',
    'mission_nightly_evolution',
  ]);

  requireAll(diagnostics, files.handlers, sources.handlers, [
    'crate::feature_gates::optional_feature_for_tool(name)',
    'crate::feature_gates::disabled_tool_result(name, feature)',
  ]);

  requireAll(diagnostics, files.main, sources.main, [
    'mod feature_gates;',
    'feature_gates::optional_feature_enabled(feature_gates::WORKFLOW_ENV)',
    'workflow_run startup recovery disabled in kernel-core mode',
    'feature_gates::optional_feature_enabled(feature_gates::SELF_EVOLUTION_ENV)',
    'self-evolution services disabled in kernel-core mode',
    'feature_gates::optional_feature_enabled(feature_gates::MEMORY_ENV)',
    'embedding worker disabled in kernel-core mode',
    'AST embedding health monitor disabled in kernel-core mode',
    'feature_gates::optional_feature_enabled(feature_gates::ROUTER_EXPERIMENTS_ENV)',
    'Gemini logger worker disabled in kernel-core mode',
    'vision worker disabled in kernel-core mode',
  ]);

  requireAll(diagnostics, files.deployDaemon, sources.deployDaemon, [
    'MISSIOND_FULL_OS_ENABLE',
    'MISSIOND_FEATURE_WORKFLOW_ENABLE',
    'MISSIOND_FEATURE_MEMORY_ENABLE',
    'MISSIOND_FEATURE_SKILL_STORE_ENABLE',
    'MISSIOND_FEATURE_ROUTER_EXPERIMENTS_ENABLE',
    'MISSIOND_FEATURE_CODEX_REPLAY_ENABLE',
    'MISSIOND_FEATURE_SELF_EVOLUTION_ENABLE',
    'MISSIOND_FEATURE_CONVERSATIONS_ENABLE',
    'MISSIOND_FEATURE_INFRA_OS_ENABLE',
    'MISSIOND_FEATURE_BOARD_ADVANCED_ENABLE',
    'plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FULL_OS_ENABLE"',
  ]);

  requireAll(diagnostics, files.migration, sources.migration, [
    'CREATE TABLE IF NOT EXISTS capability_grants',
    "'read', 'write', 'claim', 'settle', 'delegate', 'deploy', 'network'",
    'CREATE TABLE IF NOT EXISTS capability_audit_events',
    'CREATE TABLE IF NOT EXISTS jobs',
    'CREATE TABLE IF NOT EXISTS job_attempts',
    'CREATE TABLE IF NOT EXISTS work_leases',
    'CREATE TABLE IF NOT EXISTS review_gates',
    'CREATE TABLE IF NOT EXISTS board_task_views',
    'CREATE OR REPLACE VIEW completion_artifacts AS',
    'uq_work_leases_active_scope',
    'uq_shared_claims_active_scope',
    "WHERE status = 'active'",
  ]);

  requireAll(diagnostics, files.capabilityGrantOperationMigration, sources.capabilityGrantOperationMigration, [
    'DROP CONSTRAINT IF EXISTS capability_grants_operation_check',
    'ADD CONSTRAINT capability_grants_operation_check',
    "'read', 'write', 'claim', 'settle', 'delegate', 'deploy', 'network', 'spawn'",
  ]);

  requireAll(diagnostics, files.kernelReverseMigration, sources.kernelReverseMigration, [
    'DROP CONSTRAINT IF EXISTS capability_grants_subject_kind_check',
    "'worker', 'conversation', 'task', 'system', 'operator', 'daemon'",
    'CREATE TABLE IF NOT EXISTS task_contracts',
    'ADD COLUMN IF NOT EXISTS heartbeat_at',
    'ADD COLUMN IF NOT EXISTS attempt_id',
    'producer_subject_kind',
    'capability_grant_id',
    'DROP VIEW IF EXISTS completion_artifacts',
    'CREATE VIEW completion_artifacts AS',
  ]);

  requireAll(diagnostics, files.sharedMemory, sources.sharedMemory, [
    'const EVIDENCE_REQUIRED_CODE: &str = "EVIDENCE_REQUIRED";',
    'const COMPLETION_ARTIFACT_INVALID_CODE: &str = "COMPLETION_ARTIFACT_INVALID";',
    'const CAPABILITY_DENIED_CODE: &str = "CAPABILITY_DENIED";',
    'const RUNTIME_METADATA_REQUIRED_CODE: &str = "RUNTIME_METADATA_REQUIRED";',
    'const TASK_CONTRACT_REQUIRED_CODE: &str = "TASK_CONTRACT_REQUIRED";',
    'const WRITE_SCOPE_VIOLATION_CODE: &str = "WRITE_SCOPE_VIOLATION";',
    'fn kernel_routed_action_error',
    'shared memory action `{action}` must enter through ControlPlaneKernel',
    'struct CapabilityGrantInput',
    'pub(crate) struct CapabilityCheckRequest',
    'pub(crate) struct TaskRuntimeContract',
    'upsert_task_contract_from_metadata',
    'ensure_task_contract_from_metadata',
    'ON CONFLICT (task_id) DO NOTHING',
    'pub(crate) async fn grant_task_capabilities',
    'pub(crate) async fn claim_lease_typed',
    'pub(crate) async fn task_result_put_typed',
    'pub(crate) async fn task_result_put_command',
    'pub(crate) fn task_result_put_request_from_args',
    'pub(crate) async fn audit_capability_bypass',
    '"record_job_event"',
    'INSERT INTO capability_grants',
    'INSERT INTO capability_audit_events',
    'INSERT INTO task_contracts',
    'INSERT INTO board_task_views',
    'async fn require_capability',
    'pub(crate) async fn task_runtime_contract',
    'pub(crate) async fn update_task_contract_capability_grants',
    'pub(crate) async fn settle_worker_command',
    'pub(crate) async fn active_capability_grant_id',
    'pub(crate) struct TaskResultPutRequest',
    'pub(crate) struct WorkerSettleRequest',
    'FROM task_contracts',
    'no exact active subject-bound capability grant',
    'ensure_optional_feature_enabled_for_shared_action',
    'ensure_workflow_enabled_for_shared_action',
    'ensure_router_experiments_enabled_for_shared_action',
    'workflow runs, checkpoints, plan DAG, review gate, and swarm orchestration are full-os optional layers',
    'model_route_outcomes and route learning are non-core projections',
    'async fn verify_completion_scope',
    'fn attempt_actual_changed_paths',
    'fn git_changed_paths_between',
    'worktree_manifests phase=pre',
    'verifier": "attempt baseline diff"',
    'git diff --name-only',
    'operation: "write".to_string()',
    'operation: "claim".to_string()',
    'operation: "settle".to_string()',
    'attempt_id',
    'operation: "spawn"',
    'worker_settle(done) for task {task_id} requires artifact_hash',
    'artifact_hash {artifact_hash} is not a completed task-result-artifact for task {task_id}',
    '"artifact.accepted"',
    '"settle.requested"',
    '"job.completed"',
    '"job.blocked"',
    '"job.failed"',
    'source": "job_state_machine"',
    'SELECT pg_advisory_xact_lock(hashtextextended($1::text || \':\' || $2::text, 0))',
    'mission_shared_memory claim system/operator authority',
    'mission_shared_memory release system/operator authority',
    'mission_shared_memory heartbeat system/operator authority',
    'FOR UPDATE',
    'INSERT INTO work_leases',
    'FROM work_leases',
    '"code": CLAIM_CONFLICT_CODE',
  ]);

  requireAll(diagnostics, files.controlPlaneKernel, sources.controlPlaneKernel, [
    'pub(crate) struct ControlPlaneKernel',
    'pub(crate) struct SettleTaskCommand',
    'pub(crate) struct RecordObservationCommand',
    'pub(crate) struct StartAttemptCommand',
    'pub(crate) struct JobEventCommand',
    'pub(crate) struct CapabilityGrantCommand',
    'pub(crate) struct ClaimLeaseCommand',
    'pub(crate) struct ReleaseLeaseCommand',
    'pub(crate) struct HeartbeatLeaseCommand',
    'pub(crate) struct RequireCapabilityCommand',
    'pub(crate) struct GrantTaskCapabilitiesCommand',
    'pub(crate) struct UpsertTaskContractCommand',
    'pub(crate) struct UpdateTaskContractCapabilityGrantsCommand',
    'pub(crate) async fn record_observation',
    'pub(crate) async fn record_observation_command',
    'pub(crate) async fn start_attempt_command',
    'pub(crate) async fn write_completion_artifact',
    'pub(crate) async fn task_result_put_command',
    'pub(crate) async fn settle_task',
    'pub(crate) async fn settle_task_command',
    'pub(crate) async fn worker_settle_command',
    'pub(crate) async fn claim_lease',
    'pub(crate) async fn claim_lease_command',
    'pub(crate) async fn release_lease_command',
    'pub(crate) async fn heartbeat_lease_command',
    'pub(crate) async fn require_capability',
    'pub(crate) async fn require_capability_command',
    'pub(crate) async fn capability_check_command',
    'pub(crate) async fn capability_grant_command',
    'pub(crate) async fn job_event_command',
    'pub(crate) async fn grant_task_capabilities_command',
    'pub(crate) async fn upsert_task_contract_command',
    'pub(crate) async fn update_task_contract_capability_grants_command',
    'pub(crate) async fn project_board_view',
    'pub(crate) async fn complete_system_task',
    'CapabilityCheckRequest',
    'WorkerSettleRequest',
    'record_job_event_typed',
    'settle_worker_command',
    'job_event_typed',
    'grant_task_capabilities',
    'upsert_task_contract_from_metadata',
    'update_task_contract_capability_grants',
    'claim_lease_typed',
    'ensure_task_contract_from_metadata',
    'TaskCompletionEvidenceWriter::new',
    'artifact_hash: Some(artifact_hash.to_string())',
  ]);
  rejectAll(diagnostics, files.controlPlaneKernel, sources.controlPlaneKernel, [
    'task.runtime_metadata.clone()',
  ]);

  requireAll(diagnostics, files.evidenceWriter, sources.evidenceWriter, [
    'into_task_result_put_args',
    '.task_result_put_request_from_args(&payload)',
    '.task_result_put_command(request)',
  ]);
  rejectAll(diagnostics, files.evidenceWriter, sources.evidenceWriter, [
    '.handle_action(&payload)',
    'fn into_payload',
  ]);

  requireAll(diagnostics, files.autopilot, sources.autopilot, [
    'missiond.task-result-candidate.v1',
    'completed_task_result_artifact_hash_for_task',
    'settle_autopilot_done_from_existing_artifact',
    'canonical artifact remains the only close authority',
    'canonical completed task_result_artifact hash required',
    'no canonical completed task_result_artifact exists yet',
    '.record_observation_command(RecordObservationCommand',
    '.task_result_get_typed(&args)',
    'ControlPlaneKernel::new(state)',
    '.settle_task_command(SettleTaskCommand',
    '.active_capability_grant_id(',
    '.grant_task_capabilities_command(GrantTaskCapabilitiesCommand',
    '.update_task_contract_capability_grants_command(',
    '.task_runtime_contract(task.id.as_str())',
    'task_contract_workstation_class(task, runtime_contract)',
    'autopilot_grounding_gate_reason_from_contract(&task, &runtime_contract)',
    'task_contract_runtime_envelope(runtime_contract)',
    'durable_provider_completion_for_slot_task(state, task, &slot_id, &runtime_contract)',
    'output_contract_close_blocker_for_contract(',
    'BoardTask.runtime_metadata is UI cache only',
    'task_contracts completion_materialization_policy is not autopilot_readonly_ok',
    'autopilot_readonly_ok',
    'observed_readonly_completion',
  ]);
  rejectAll(diagnostics, files.autopilot, sources.autopilot, [
    '"action": "job_event"',
    '"action": "task_result_get"',
    '"action": "task_result_put"',
    '"action": "worker_settle"',
    'implicit_jarvis_readonly_interaction',
    'board_task_runtime_metadata_string(task, "completion_materialization_policy")',
    'runtime_contract.contains("## Swarm metadata")',
    'runtime_contract.contains("## Dispatch metadata")',
    'let task_class = board_task_workstation_class(task);',
    'extract_board_task_dispatch_metadata_field(task, "engine_hint")',
    'extract_board_task_dispatch_metadata_field(task, "pool_hint")',
  ]);

  requireAll(diagnostics, files.flowEngine, sources.flowEngine, [
    '.task_runtime_contract(task.id.as_str())',
    'legacy BoardTask.description fallback is disabled',
  ]);
  rejectAll(diagnostics, files.flowEngine, sources.flowEngine, [
    'extract_task_metadata_field(&task.description',
    'fn extract_task_metadata_field',
    '## Dispatch metadata',
    '## Swarm metadata',
  ]);

  requireAll(diagnostics, files.taskDelegate, sources.taskDelegate, [
    'grant_task_capabilities',
    'GrantTaskCapabilitiesCommand',
    'capability_grant_ids',
    'sandbox_profile',
    'task_contract_id',
    'Some(&task_id)',
    'Some(&capability_grant_ids)',
    'preallocated_slot_id',
    'subject_kind: "worker".to_string()',
    'create_args["subject_kind"] = Value::String("worker".to_string())',
    'runtime_metadata: Some(runtime_metadata.clone())',
    'control_state": "task_contracts"',
    'UpsertTaskContractCommand',
    '.task_runtime_contract(task.id.as_str())',
    'task_contract_references_parent(&contract, parent_id)',
    'task_contract_references_source(&contract, src)',
    'contract.write_scope.clone()',
    'duplicate_worker_source_reference_uses_task_contracts',
    'fn enrich_runtime_metadata_with_control_facts',
    'fn sandbox_profile_for_worker',
    '.start_attempt_command(StartAttemptCommand',
    '.record_observation_command(RecordObservationCommand',
    '.workflow_start_typed(&json!',
    '.workflow_checkpoint_typed(&json!',
    'TaskCompletionEvidenceInput',
    '.write_completion_artifact(',
    'ControlPlaneKernel::new(&state)',
    '.settle_task_command(SettleTaskCommand',
    '.release_lease_command(ReleaseLeaseCommand',
  ]);
  rejectAll(diagnostics, files.taskDelegate, sources.taskDelegate, [
    '"action": "workflow_start"',
    '"action": "workflow_checkpoint"',
    '"action": "task_result_put"',
    '"action": "worker_settle"',
    'fn parse_write_scope_from_description',
    'fn description_references_source',
  ]);

  requireAll(diagnostics, files.agentExecutionClaimLease, sources.agentExecutionClaimLease, [
    'ControlPlaneKernel::new(state)',
    '.claim_lease_command(ClaimLeaseCommand',
    ':work-lease-id {work_lease_id}',
    '"work_lease_id": work_lease_id',
  ]);
  rejectAll(diagnostics, files.agentExecutionClaimLease, sources.agentExecutionClaimLease, [
    '.claim_lease_typed(',
  ]);

  requireAll(diagnostics, files.agentExecutionClaimRelease, sources.agentExecutionClaimRelease, [
    'ControlPlaneKernel::new(state)',
    '.release_lease_command(ReleaseLeaseCommand',
    'work-lease-id',
    'mission_execution.release',
    'claim {} has no canonical work_leases id',
    'work lease {} was not released',
  ]);

  requireAll(diagnostics, files.agentExecutionClaimHeartbeat, sources.agentExecutionClaimHeartbeat, [
    'ControlPlaneKernel::new(state)',
    '.heartbeat_lease_command(HeartbeatLeaseCommand',
    'work-lease-id',
    'mission_execution.heartbeat',
    'claim {} has no canonical work_leases id',
    'work lease {} was not heartbeated',
  ]);

  requireAll(diagnostics, files.boardCreateHandler, sources.boardCreateHandler, [
    'ControlPlaneKernel::new(state)',
    '.grant_task_capabilities_command(GrantTaskCapabilitiesCommand',
    '.upsert_task_contract_command(UpsertTaskContractCommand',
  ]);
  rejectAll(diagnostics, files.boardCreateHandler, sources.boardCreateHandler, [
    '.grant_task_capabilities(',
    '.upsert_task_contract_from_metadata(',
  ]);

  requireAll(diagnostics, files.v2Subscribers, sources.v2Subscribers, [
    'runtime_metadata: Some(runtime_metadata)',
    '"source": "eventbridge"',
    '"control_state": "task_contracts"',
    '"task_class": "deploy-ops"',
    '"task_class": "router-ops"',
    '"pool_hint": "claude-code-deploy-ops"',
    '"pool_hint": "claude-code-default"',
    '"sandbox_profile": "read-only"',
    'deployment_event_response_task_declares_deploy_ops_lane',
    'router_event_response_task_declares_runtime_contract',
  ]);

  requireAll(diagnostics, files.computeSlot, sources.computeSlot, [
    'RequireCapabilityCommand',
    'ControlPlaneKernel::new(state)',
    '.require_capability_command(RequireCapabilityCommand',
    'operation: "spawn".to_string()',
    'capability_grant_id',
    'unwrap_or("worker")',
    'unwrap_or(&slot_id)',
    'mission_compute_slot(create) requires task_id with an active worker-bound spawn capability',
    'task_runtime_contract(task_id)',
    'sandbox override does not match canonical task_contracts sandbox_profile',
    'error_codes::SANDBOX_POLICY_UNSUPPORTED',
    'audit_capability_bypass',
    'error_codes::CAPABILITY_DENIED',
  ]);

  requireAll(diagnostics, files.ptyHandler, sources.ptyHandler, [
    'RequireCapabilityCommand',
    'ControlPlaneKernel::new(state)',
    '.require_capability_command(RequireCapabilityCommand',
    'operation: "spawn".to_string()',
    'mission_pty_spawn requires task_id with an exact spawn capability',
    'task_runtime_contract(task_id)',
    'mission_pty_spawn slot sandbox does not match canonical task_contracts sandbox_profile',
    'audit_capability_bypass',
    'operator confirmed mission_pty_spawn without worker-bound spawn grant',
    'error_codes::SANDBOX_POLICY_UNSUPPORTED',
    'error_codes::CAPABILITY_DENIED',
  ]);

  requireAll(diagnostics, files.spawner, sources.spawner, [
    'enforce_spawn_sandbox_policy(pty_slot, &mut options)?',
    'fn enforce_spawn_sandbox_policy',
    'workspace-write',
    'dangerously_skip_permissions = false',
    'MISSIOND_ALLOW_BROAD_SKIP_PERMISSIONS',
    'SANDBOX_POLICY_UNSUPPORTED',
  ]);

  requireAll(diagnostics, files.boardStore, sources.boardStore, [
    'fn board_task_contract_projection',
    'fn board_task_runtime_metadata',
    'async fn upsert_task_contract_projection_tx',
    'async fn insert_board_task',
    'control_state".to_string())',
    'serde_json::json!("task_contracts")',
    'INSERT INTO task_contracts',
    'ON CONFLICT (task_id)',
    'tx.commit().await?',
    'artifact_hash = $2',
    'runtime_metadata = $',
    "SELECT pg_advisory_xact_lock(hashtextextended($1::text || ':' || $2::text, 0))",
    'FOR UPDATE',
    'INSERT INTO work_leases',
    "scope_kind = 'board_task'",
    "UPDATE work_leases\n            SET status = 'released'",
    'DbError::ClaimConflict',
  ]);
  rejectAll(diagnostics, files.boardStore, sources.boardStore, [
    'ON CONFLICT DO NOTHING',
  ]);

  requireAll(diagnostics, files.boardTypes, sources.boardTypes, [
    'pub runtime_metadata: Option<serde_json::Value>',
    'pub artifact_hash: Option<String>',
  ]);

  requireAll(diagnostics, files.verifierRouterMigration, sources.verifierRouterMigration, [
    'CREATE TABLE IF NOT EXISTS worktree_manifests',
    'CREATE TABLE IF NOT EXISTS model_route_outcomes',
    'prompt_tokens',
    'completion_tokens',
    'cost_usd',
    'decision JSONB',
    'outcome JSONB',
    'status TEXT',
    'idx_worktree_manifests_attempt_phase',
    'idx_model_route_outcomes_model',
  ]);

  requireAll(diagnostics, files.capabilityGrantOperationMigration, sources.capabilityGrantOperationMigration, [
    'DROP CONSTRAINT IF EXISTS capability_grants_operation_check',
    'ADD CONSTRAINT capability_grants_operation_check',
    "'read', 'write', 'claim', 'settle', 'delegate', 'deploy', 'network', 'spawn'",
  ]);

  requireAll(diagnostics, files.backfillRuntimeMetadata, sources.backfillRuntimeMetadata, [
    'backfill-board-runtime-metadata',
    '--apply',
    'runtime_metadata',
    'task_contracts',
    "control_state: 'task_contracts'",
    'taskContractForBackfill',
    'ON CONFLICT (task_id)',
    'capability_grants',
    'parseLegacyDescription',
  ]);
  rejectAll(diagnostics, files.backfillRuntimeMetadata, sources.backfillRuntimeMetadata, [
    "control_state: 'runtime_metadata'",
  ]);

  for (const fileKey of ['dbError', 'mcpTools', 'mcpGateway', 'boardHandler']) {
    requireAll(diagnostics, files[fileKey], sources[fileKey], [
      'EVIDENCE_REQUIRED',
      'CLAIM_CONFLICT',
      'COMPLETION_ARTIFACT_INVALID',
      'CAPABILITY_DENIED',
      'RUNTIME_METADATA_REQUIRED',
      'TASK_CONTRACT_REQUIRED',
      'SANDBOX_POLICY_UNSUPPORTED',
      'WRITE_SCOPE_VIOLATION',
    ]);
  }

  requireAll(diagnostics, files.sharedHandler, sources.sharedHandler, [
    'StructuredControlError',
    'control.code',
    'with_details(control.details.clone())',
    'with_suggestion(suggestion.clone())',
    'Board notes and PTY text are projections only',
    'ControlPlaneKernel::new(state)',
    '.task_result_put_request_from_args(&args)',
    '.task_result_put_command(request)',
    '.worker_settle_request_from_args(&args)',
    '.worker_settle_command(request)',
    '.claim_lease_command(claim_lease_command_from_args(&args)?)',
    '.release_lease_command(release_lease_command_from_args(&args)?)',
    '.heartbeat_lease_command(heartbeat_lease_command_from_args(&args)?)',
    '.capability_check_command(capability_check_command_from_args(&args)?)',
    'CapabilityGrantCommand { args: args.clone() }',
    '.capability_grant_command(',
    'JobEventCommand { args: args.clone() }',
    '.job_event_command(',
  ]);
  rejectAll(diagnostics, files.sharedHandler, sources.sharedHandler, [
    'task_result_put_typed(&args)',
    'settle_worker_typed(args.clone())',
    'capability_check_typed(&args)',
    'capability_grant_from_args(&args)',
    'job_event_typed(args.clone())',
    'state.shared_memory.claim_typed(&args)',
    'state.shared_memory.release_typed(args.clone())',
    'state.shared_memory.heartbeat_typed(args.clone())',
  ]);

  requireAll(diagnostics, files.boardRoute, sources.boardRoute, [
    'missiondBody?.code ?? missiondBody?.error_code',
    "code !== 'EVIDENCE_REQUIRED'",
  ]);
  rejectAll(diagnostics, files.boardRoute, sources.boardRoute, [
    '.includes(',
    '.startsWith(',
    'JSON.stringify(resp.error)',
  ]);

  requireAll(diagnostics, files.boardStoreTs, sources.boardStoreTs, [
    "code === 'EVIDENCE_REQUIRED'",
    "code === 'CLAIM_CONFLICT'",
    "code === 'CAPABILITY_DENIED'",
    "code === 'WRITE_SCOPE_VIOLATION'",
    "code === 'RUNTIME_METADATA_REQUIRED'",
  ]);

  rejectDirectSettleOutsideKernel(diagnostics, files, sources);
  rejectDirectEvidenceWriterOutsideKernel(diagnostics, files, sources);
  rejectDirectLeaseCommandsOutsideKernel(diagnostics, files, sources);
  rejectDirectJobEventsOutsideKernel(diagnostics, files, sources);
  rejectDirectCapabilityChecksOutsideKernel(diagnostics, files, sources);
  rejectDirectCapabilityGrantsOutsideKernel(diagnostics, files, sources);
  rejectDirectTaskContractWritesOutsideKernel(diagnostics, files, sources);

  return diagnostics;
}

function rejectDirectTaskContractWritesOutsideKernel(diagnostics, files, sources) {
  const allowed = new Set(['controlPlaneKernel', 'sharedMemory']);
  for (const [key, source] of Object.entries(sources)) {
    if (allowed.has(key)) continue;
    if (
      source.includes('.upsert_task_contract_from_metadata(') ||
      source.includes('.update_task_contract_capability_grants(')
    ) {
      diagnostics.push({
        file: files[key],
        message: 'direct task_contract writes outside ControlPlaneKernel are forbidden',
      });
    }
  }
}

function rejectDirectCapabilityGrantsOutsideKernel(diagnostics, files, sources) {
  const allowed = new Set(['controlPlaneKernel', 'sharedMemory']);
  for (const [key, source] of Object.entries(sources)) {
    if (allowed.has(key)) continue;
    if (source.includes('.grant_task_capabilities(')) {
      diagnostics.push({
        file: files[key],
        message: 'direct task capability grants outside ControlPlaneKernel are forbidden',
      });
    }
  }
}

function rejectDirectCapabilityChecksOutsideKernel(diagnostics, files, sources) {
  const allowed = new Set(['controlPlaneKernel', 'sharedMemory', 'sharedHandler']);
  for (const [key, source] of Object.entries(sources)) {
    if (allowed.has(key)) continue;
    if (source.includes('.require_capability(')) {
      diagnostics.push({
        file: files[key],
        message: 'direct shared_memory capability checks outside ControlPlaneKernel are forbidden',
      });
    }
  }
}

function rejectDirectJobEventsOutsideKernel(diagnostics, files, sources) {
  const allowed = new Set(['controlPlaneKernel', 'sharedMemory', 'sharedHandler']);
  for (const [key, source] of Object.entries(sources)) {
    if (allowed.has(key)) continue;
    if (source.includes('.job_event_typed(') || source.includes('.record_job_event_typed(')) {
      diagnostics.push({
        file: files[key],
        message: 'direct shared_memory job-event writes outside ControlPlaneKernel are forbidden',
      });
    }
  }
}

function rejectDirectLeaseCommandsOutsideKernel(diagnostics, files, sources) {
  const allowed = new Set(['controlPlaneKernel', 'sharedMemory']);
  for (const [key, source] of Object.entries(sources)) {
    if (allowed.has(key)) continue;
    if (
      source.includes('.claim_lease_typed(') ||
      source.includes('.claim_typed(') ||
      source.includes('.release_typed(') ||
      source.includes('.heartbeat_typed(')
    ) {
      diagnostics.push({
        file: files[key],
        message: 'direct shared_memory lease commands outside ControlPlaneKernel are forbidden',
      });
    }
  }
}

function rejectDirectSettleOutsideKernel(diagnostics, files, sources) {
  for (const [key, source] of Object.entries(sources)) {
    if (key === 'controlPlaneKernel' || key === 'sharedMemory') continue;
    if (source.includes('.settle_worker_command(')) {
      diagnostics.push({
        file: files[key],
        message: 'direct shared_memory.settle_worker_command outside ControlPlaneKernel is forbidden',
      });
    }
  }
}

function rejectDirectEvidenceWriterOutsideKernel(diagnostics, files, sources) {
  for (const [key, source] of Object.entries(sources)) {
    if (key === 'controlPlaneKernel' || key === 'evidenceWriter') continue;
    if (source.includes('TaskCompletionEvidenceWriter::new')) {
      diagnostics.push({
        file: files[key],
        message: 'direct TaskCompletionEvidenceWriter construction outside ControlPlaneKernel is forbidden',
      });
    }
  }
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required text: ${needle}` });
    }
  }
}

function rejectAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden text present: ${needle}` });
    }
  }
}

main();
