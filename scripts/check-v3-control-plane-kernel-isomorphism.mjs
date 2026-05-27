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
`;

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  migration: 'crates/missiond-core/migrations/20260527000000_control_plane_kernel.sql',
  sharedMemory: 'crates/missiond-daemon/src/engine/shared_memory.rs',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  taskDelegate: 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
  spawner: 'crates/missiond-daemon/src/slot_orchestrator/spawner.rs',
  boardStore: 'crates/missiond-core/src/db/pg/board.rs',
  boardTypes: 'crates/missiond-core/src/types/board.rs',
  dbError: 'crates/missiond-core/src/db/error.rs',
  mcpTools: 'crates/missiond-mcp/src/tools/mod.rs',
  mcpGateway: 'crates/missiond-mcp/src/gen_gateway.rs',
  boardHandler: 'crates/missiond-daemon/src/handlers/knowledge/board.rs',
  sharedHandler: 'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs',
  boardRoute: 'packages/board/src/app/api/tasks/route.ts',
  boardStoreTs: 'packages/board/src/store.ts',
  verifierRouterMigration: 'crates/missiond-core/migrations/20260527001000_runtime_verifier_router_outcomes.sql',
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
    ':facts [task_result_artifacts event_log jobs job_attempts work_leases capability_grants capability_audit_events review_gates board_task_views]',
    ':runtime-abi-fields [completion_artifact_schema job_state_machine capability_policy sandbox_policy projection_policy]',
    ':hard-cutover true',
    'BoardTask description, Board notes, PTY screens, TUI summaries, and provider prose are projection/observation inputs only.',
    'Missing runtime_metadata on a control-plane task returns RUNTIME_METADATA_REQUIRED',
    'task_result_put and worker_settle MUST pass capability checks for task settle',
    'Worker spawn MUST project sandbox_profile from capability/write_scope facts',
    'ProjectionEngine updates board_task_views and Board-facing status from typed events/state',
    ':checker "scripts/check-v3-control-plane-kernel-isomorphism.mjs"',
    'node scripts/check-v3-control-plane-kernel-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.migration, sources.migration, [
    'CREATE TABLE IF NOT EXISTS capability_grants',
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

  requireAll(diagnostics, files.sharedMemory, sources.sharedMemory, [
    'const EVIDENCE_REQUIRED_CODE: &str = "EVIDENCE_REQUIRED";',
    'const COMPLETION_ARTIFACT_INVALID_CODE: &str = "COMPLETION_ARTIFACT_INVALID";',
    'const CAPABILITY_DENIED_CODE: &str = "CAPABILITY_DENIED";',
    'const RUNTIME_METADATA_REQUIRED_CODE: &str = "RUNTIME_METADATA_REQUIRED";',
    'const WRITE_SCOPE_VIOLATION_CODE: &str = "WRITE_SCOPE_VIOLATION";',
    'struct CapabilityGrantInput',
    'struct TaskRuntimeContract',
    'pub(crate) async fn grant_task_capabilities',
    '"job_event" | "record_job_event"',
    'INSERT INTO capability_grants',
    'INSERT INTO capability_audit_events',
    'INSERT INTO board_task_views',
    'async fn require_capability',
    'async fn task_runtime_contract',
    'async fn verify_completion_scope',
    'self.require_capability(&task_id, "write", "task", &task_id)',
    'self.require_capability(task_id, "settle", "task", task_id)',
    'worker_settle(done) for task {task_id} requires artifact_hash',
    'artifact_hash {artifact_hash} is not a completed task-result-artifact for task {task_id}',
    '"artifact.accepted"',
    '"settle.requested"',
    '"job.completed"',
    '"job.blocked"',
    '"job.failed"',
    'source": "job_state_machine"',
    'SELECT pg_advisory_xact_lock(hashtextextended($1::text || \':\' || $2::text, 0))',
    'FOR UPDATE',
    'INSERT INTO work_leases',
    '"code": CLAIM_CONFLICT_CODE',
  ]);

  requireAll(diagnostics, files.autopilot, sources.autopilot, [
    'missiond.task-result-candidate.v1',
    'completed_task_result_artifact_hash_for_task',
    'settle_autopilot_done_from_existing_artifact',
    'canonical artifact remains the only close authority',
    'canonical completed task_result_artifact hash required',
    'no canonical completed task_result_artifact exists yet',
    '"action": "job_event"',
    '"action": "worker_settle"',
  ]);
  rejectAll(diagnostics, files.autopilot, sources.autopilot, [
    '"action": "task_result_put"',
  ]);

  requireAll(diagnostics, files.taskDelegate, sources.taskDelegate, [
    'grant_task_capabilities',
    'capability_grant_ids',
    'sandbox_profile',
    'task_contract_id',
    'runtime_metadata: Some(runtime_metadata.clone())',
    'runtime_metadata: Some(runtime_metadata)',
    'control_state": "runtime_metadata"',
    'fn enrich_runtime_metadata_with_control_facts',
    'fn sandbox_profile_for_worker',
    '#[cfg(test)]\nfn parse_write_scope_from_description',
    '#[cfg(test)]\nfn description_references_source',
    'board_task_source_reference_uses_runtime_metadata_without_description_fallback',
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
    'artifact_hash = $2',
    'runtime_metadata = $',
    'SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))',
    'FOR UPDATE',
    'INSERT INTO work_leases',
    'DbError::ClaimConflict',
  ]);

  requireAll(diagnostics, files.boardTypes, sources.boardTypes, [
    'pub runtime_metadata: Option<serde_json::Value>',
    'pub artifact_hash: Option<String>',
  ]);

  requireAll(diagnostics, files.verifierRouterMigration, sources.verifierRouterMigration, [
    'CREATE TABLE IF NOT EXISTS worktree_manifests',
    'CREATE TABLE IF NOT EXISTS model_route_outcomes',
    'idx_worktree_manifests_attempt_phase',
    'idx_model_route_outcomes_model',
  ]);

  requireAll(diagnostics, files.backfillRuntimeMetadata, sources.backfillRuntimeMetadata, [
    'backfill-board-runtime-metadata',
    '--apply',
    'runtime_metadata',
    'capability_grants',
    'parseLegacyDescription',
  ]);

  for (const fileKey of ['dbError', 'mcpTools', 'mcpGateway', 'boardHandler']) {
    requireAll(diagnostics, files[fileKey], sources[fileKey], [
      'EVIDENCE_REQUIRED',
      'CLAIM_CONFLICT',
      'COMPLETION_ARTIFACT_INVALID',
      'CAPABILITY_DENIED',
      'RUNTIME_METADATA_REQUIRED',
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

  return diagnostics;
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
