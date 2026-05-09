#!/usr/bin/env node

// V3 per-surface checker for the durable shared-memory runtime.
//
// Pinned authority:
//   - .missiond/v3/missiond-blueprint.lisp
//       * (function mission-shared-memory ...)
//       * (function semantic-ir-compiler ...)
//       * (surface mission-shared-memory ...)
//       * (surface semantic-ir-compiler ...)
//   - .missiond/v2/intent-tools.lisp (module shared_memory ...)
//   - .missiond/tasks/schema/shared-memory-v1.lisp (compatibility projection)
//   - .missiond/workflows/semantic-ir-shared-memory-convergence.lisp
//   - tools/missiond_lispc/bin/{emit_json,main}.ml
//   - scripts/compile-v3-runtime.mjs
//   - scripts/check-task-memory.mjs (legacy ledger checker, now a compat note)
//   - scripts/check-v3-code-isomorphism-complete.mjs (aggregate registration)
// Pinned implementation:
//   - crates/missiond-core/migrations/20260508000000_shared_memory.sql
//   - crates/missiond-core/migrations/20260509000000_execution_control_plane.sql
//   - crates/missiond-daemon/src/engine/shared_memory.rs
//   - crates/missiond-daemon/src/engine/mod.rs
//   - crates/missiond-daemon/src/engine/master_control.rs
//   - crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs
//   - crates/missiond-daemon/src/handlers/mod.rs
//   - crates/missiond-daemon/src/handlers/knowledge/mod.rs
//   - crates/missiond-daemon/src/state.rs
//   - crates/missiond-daemon/src/main.rs
//   - crates/missiond-daemon/src/handlers/compute/task_delegate.rs
//   - crates/missiond-mcp/src/tools/knowledge/shared_memory.rs
//   - crates/missiond-mcp/src/tools/mod.rs
//   - crates/missiond-mcp/src/tools/knowledge/mod.rs
//
// Read-only. The checker does not exercise the database; it is a textual
// isomorphism contract between the Lisp SSOT and the implementation files
// that the V3 surface mission-shared-memory points at.

import fs from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();

const FILES = [
  '.missiond/v3/missiond-blueprint.lisp',
  '.missiond/v2/intent-tools.lisp',
  '.missiond/tasks/schema/shared-memory-v1.lisp',
  '.missiond/workflows/semantic-ir-shared-memory-convergence.lisp',
  'tools/missiond_lispc/bin/emit_json.ml',
  'tools/missiond_lispc/bin/main.ml',
  'scripts/compile-v3-runtime.mjs',
  'scripts/check-task-memory.mjs',
  'scripts/check-v3-code-isomorphism-complete.mjs',
  'crates/missiond-core/migrations/20260508000000_shared_memory.sql',
  'crates/missiond-core/migrations/20260509000000_execution_control_plane.sql',
  'crates/missiond-daemon/src/engine/shared_memory.rs',
  'crates/missiond-daemon/src/engine/mod.rs',
  'crates/missiond-daemon/src/engine/master_control.rs',
  'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs',
  'crates/missiond-daemon/src/handlers/mod.rs',
  'crates/missiond-daemon/src/handlers/knowledge/mod.rs',
  'crates/missiond-daemon/src/state.rs',
  'crates/missiond-daemon/src/main.rs',
  'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
  'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs',
  'crates/missiond-mcp/src/tools/mod.rs',
  'crates/missiond-mcp/src/tools/knowledge/mod.rs',
];

// Each entry: [diagnostic id, repo-relative file, exact substring needle].
// Needles are exact-substring; ordered from authoritative SSOT to runtime
// projections so the failure log reads top-down.
const NEEDLES = [
  // Blueprint surface + function declarations.
  ['blueprint surface', '.missiond/v3/missiond-blueprint.lisp', '(surface mission-shared-memory'],
  ['blueprint function', '.missiond/v3/missiond-blueprint.lisp', '(function mission-shared-memory'],
  ['blueprint surface entry tools', '.missiond/v3/missiond-blueprint.lisp', '[mission_shared_memory mission_context_slice mission_claim_status'],
  ['blueprint compatibility note', '.missiond/v3/missiond-blueprint.lisp', 'compatibility projection'],
  ['semantic compiler function', '.missiond/v3/missiond-blueprint.lisp', '(function semantic-ir-compiler'],
  ['semantic compiler surface', '.missiond/v3/missiond-blueprint.lisp', '(surface semantic-ir-compiler'],
  ['compression-contract checks include shared-memory checker', '.missiond/v3/missiond-blueprint.lisp', '"node scripts/check-v3-shared-memory-isomorphism.mjs"'],

  // Intent-tools catalog: 3 new MCP tools declared in the knowledge domain.
  ['intent-tools module', '.missiond/v2/intent-tools.lisp', '(module shared_memory'],
  ['intent-tools capability family', '.missiond/v2/intent-tools.lisp', 'multi-agent-shared-memory'],
  ['intent-tools tool mission_shared_memory', '.missiond/v2/intent-tools.lisp', '(tool mission_shared_memory'],
  ['intent-tools tool mission_context_slice', '.missiond/v2/intent-tools.lisp', '(tool mission_context_slice'],
  ['intent-tools tool mission_claim_status', '.missiond/v2/intent-tools.lisp', '(tool mission_claim_status'],
  ['intent-tools v3 surface link', '.missiond/v2/intent-tools.lisp', ':v3-surface "mission-shared-memory"'],
  ['intent-tools legacy projection link', '.missiond/v2/intent-tools.lisp', ':legacy-projection ".missiond/tasks/schema/shared-memory-v1.lisp"'],

  // Legacy ledger schema explicitly demoted to compatibility projection.
  ['ledger schema status', '.missiond/tasks/schema/shared-memory-v1.lisp', ':status "compatibility-projection'],
  ['ledger schema durable runtime pointer', '.missiond/tasks/schema/shared-memory-v1.lisp', '(durable-runtime missiond.shared-memory.runtime.v1'],
  ['ledger schema migration pointer', '.missiond/tasks/schema/shared-memory-v1.lisp', 'crates/missiond-core/migrations/20260508000000_shared_memory.sql'],
  ['ledger schema checker pointer', '.missiond/tasks/schema/shared-memory-v1.lisp', 'scripts/check-v3-shared-memory-isomorphism.mjs'],

  // Workflow remains the active convergence contract.
  ['workflow active', '.missiond/workflows/semantic-ir-shared-memory-convergence.lisp', ':workflow_id semantic-ir-shared-memory-convergence'],

  // Semantic IR compile chain.
  ['OCaml emit command', 'tools/missiond_lispc/bin/main.ml', 'emit-semantic-ir'],
  ['OCaml emitter', 'tools/missiond_lispc/bin/emit_json.ml', 'missiond.semantic-ir.v1'],
  ['compiled semantic output', 'scripts/compile-v3-runtime.mjs', 'compiled-semantic-ir.json'],
  ['compiled agent slices', 'scripts/compile-v3-runtime.mjs', 'compiled-agent-slices.json'],
  ['compiled workflow contracts', 'scripts/compile-v3-runtime.mjs', 'compiled-workflow-contracts.json'],

  // Legacy ledger checker carries the compatibility-projection note.
  ['legacy ledger checker compat note', 'scripts/check-task-memory.mjs', 'compatibility projection'],
  ['legacy ledger checker durable runtime pointer', 'scripts/check-task-memory.mjs', 'mission_shared_memory'],

  // Aggregate gate registers the surface and per-surface checker.
  ['aggregate expected surface', 'scripts/check-v3-code-isomorphism-complete.mjs', "'mission-shared-memory'"],
  ['aggregate per-surface checker', 'scripts/check-v3-code-isomorphism-complete.mjs', 'scripts/check-v3-shared-memory-isomorphism.mjs'],

  // PG schema: durable shared-memory tables plus execution-control tables.
  ['migration shared_events', 'crates/missiond-core/migrations/20260508000000_shared_memory.sql', 'CREATE TABLE IF NOT EXISTS shared_events'],
  ['migration shared_artifacts', 'crates/missiond-core/migrations/20260508000000_shared_memory.sql', 'CREATE TABLE IF NOT EXISTS shared_artifacts'],
  ['migration shared_claims', 'crates/missiond-core/migrations/20260508000000_shared_memory.sql', 'CREATE TABLE IF NOT EXISTS shared_claims'],
  ['migration agent_cursors', 'crates/missiond-core/migrations/20260508000000_shared_memory.sql', 'CREATE TABLE IF NOT EXISTS agent_cursors'],
  ['migration shared_events idempotency index', 'crates/missiond-core/migrations/20260508000000_shared_memory.sql', 'idx_shared_events_idempotency'],
  ['migration shared_claims active scope index', 'crates/missiond-core/migrations/20260508000000_shared_memory.sql', 'idx_shared_claims_active_scope'],
  ['execution migration workflow_runs', 'crates/missiond-core/migrations/20260509000000_execution_control_plane.sql', 'CREATE TABLE IF NOT EXISTS workflow_runs'],
  ['execution migration task_result_artifacts', 'crates/missiond-core/migrations/20260509000000_execution_control_plane.sql', 'CREATE TABLE IF NOT EXISTS task_result_artifacts'],
  ['execution migration task result artifact FK', 'crates/missiond-core/migrations/20260509000000_execution_control_plane.sql', 'REFERENCES shared_artifacts(hash)'],
  ['execution migration workflow project status index', 'crates/missiond-core/migrations/20260509000000_execution_control_plane.sql', 'idx_workflow_runs_project_status'],
  ['execution migration task artifact index', 'crates/missiond-core/migrations/20260509000000_execution_control_plane.sql', 'idx_task_result_artifacts_task'],

  // Daemon engine: SharedMemoryService surface.
  ['service struct', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'pub(crate) struct SharedMemoryService'],
  ['service ClaimRequest', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'pub(crate) struct ClaimRequest'],
  ['service handle_action', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'pub(crate) async fn handle_action'],
  ['service status_snapshot', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'pub(crate) async fn status_snapshot'],
  ['service claim_status', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'pub(crate) async fn claim_status'],
  ['service context_slice', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'pub(crate) async fn context_slice'],
  ['service claim_write_scope', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'pub(crate) async fn claim_write_scope'],
  ['service shared_events', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'shared_events'],
  ['service shared_artifacts', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'shared_artifacts'],
  ['service shared_claims', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'shared_claims'],
  ['service agent_cursors', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'agent_cursors'],
  ['service expire_stale_claims', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'expire_stale_claims'],
  ['service status schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.shared-memory.status.v1'],
  ['service claim-status schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.shared-memory.claim-status.v1'],
  ['service context-slice schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.context-slice.v1'],
  ['service event schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.shared-event.v1'],
  ['service artifact schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.shared-artifact.v1'],
  ['service claim schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.shared-claim.v1'],
  ['service claim-release schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.shared-claim-release.v1'],
  ['service claim-heartbeat schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.shared-claim-heartbeat.v1'],
  ['service cursor schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.agent-cursor.v1'],
  ['service task result action', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'task_result_put'],
  ['service task result get action', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'task_result_get'],
  ['service workflow start action', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'workflow_start'],
  ['service workflow checkpoint action', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'workflow_checkpoint'],
  ['service workflow status action', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'workflow_status'],
  ['service worker settle action', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'worker_settle'],
  ['service task result schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.task-result-artifact.v1'],
  ['service workflow schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.workflow-run.v1'],
  ['service worker settle schema', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'missiond.worker-completion-settle.v1'],
  ['service task result artifact event', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'task_result_artifact.created'],
  ['service workflow start event', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'workflow_run.started'],
  ['service worker settle event', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'worker_completion.settled'],
  ['service worker settle status guard', 'crates/missiond-daemon/src/engine/shared_memory.rs', 'normalize_worker_settle_status'],

  // Engine module registration.
  ['engine module registration', 'crates/missiond-daemon/src/engine/mod.rs', 'pub mod shared_memory;'],

  // Master-control status projection.
  ['master_control status snapshot call', 'crates/missiond-daemon/src/engine/master_control.rs', 'state.shared_memory.status_snapshot()'],
  ['master_control sharedMemory key', 'crates/missiond-daemon/src/engine/master_control.rs', '"sharedMemory"'],

  // Handler dispatcher (knowledge facade) and routing.
  ['dispatcher mission_shared_memory branch', 'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs', '"mission_shared_memory" =>'],
  ['dispatcher mission_context_slice branch', 'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs', '"mission_context_slice" =>'],
  ['dispatcher mission_claim_status branch', 'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs', '"mission_claim_status" =>'],
  ['dispatcher handle_action call', 'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs', 'state.shared_memory.handle_action'],
  ['dispatcher context_slice call', 'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs', 'state.shared_memory.context_slice'],
  ['dispatcher claim_status call', 'crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs', 'state.shared_memory.claim_status'],

  // Top-level handlers/mod.rs route to the dispatcher.
  ['handlers/mod.rs route', 'crates/missiond-daemon/src/handlers/mod.rs', '"mission_shared_memory" | "mission_context_slice" | "mission_claim_status"'],
  ['handlers/mod.rs forward', 'crates/missiond-daemon/src/handlers/mod.rs', 'shared_memory::handle(state, name, args).await'],
  ['handlers/knowledge/mod.rs registration', 'crates/missiond-daemon/src/handlers/knowledge/mod.rs', 'pub(crate) mod shared_memory;'],

  // AppState wiring + main.rs constructor.
  ['state shared_memory field', 'crates/missiond-daemon/src/state.rs', 'pub(crate) shared_memory: Arc<crate::engine::shared_memory::SharedMemoryService>'],
  ['state compatibility projection note', 'crates/missiond-daemon/src/state.rs', 'compatibility projection'],
  ['main.rs SharedMemoryService::new', 'crates/missiond-daemon/src/main.rs', 'engine::shared_memory::SharedMemoryService::new('],

  // Task delegate write-scope lease guard (claim_write_scope + lease conflict surfaces).
  ['task_delegate claim_write_scope', 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs', '.shared_memory'],
  ['task_delegate lease guard', 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs', 'SHARED_MEMORY_WRITE_LEASE_CONFLICT'],
  ['swarm lease guard', 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs', 'SWARM_SHARED_MEMORY_WRITE_LEASE_CONFLICT'],

  // MCP tool catalog (shells + registration).
  ['mcp shell mission_shared_memory', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"mission_shared_memory"'],
  ['mcp shell mission_context_slice', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"mission_context_slice"'],
  ['mcp shell mission_claim_status', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"mission_claim_status"'],
  ['mcp shell append action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"append"'],
  ['mcp shell artifact_put action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"artifact_put"'],
  ['mcp shell artifact_get action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"artifact_get"'],
  ['mcp shell claim action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"claim"'],
  ['mcp shell release action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"release"'],
  ['mcp shell heartbeat action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"heartbeat"'],
  ['mcp shell cursor action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"cursor"'],
  ['mcp shell task_result_put action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"task_result_put"'],
  ['mcp shell task_result_get action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"task_result_get"'],
  ['mcp shell workflow_start action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"workflow_start"'],
  ['mcp shell workflow_checkpoint action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"workflow_checkpoint"'],
  ['mcp shell workflow_status action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"workflow_status"'],
  ['mcp shell worker_settle action', 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs', '"worker_settle"'],
  ['mcp tools registration', 'crates/missiond-mcp/src/tools/mod.rs', 'tools.extend(shared_memory::definitions());'],
  ['mcp knowledge mod registration', 'crates/missiond-mcp/src/tools/knowledge/mod.rs', 'pub(crate) mod shared_memory;'],
];

function parseArgs(argv) {
  return { json: argv.includes('--json') };
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = [];
  for (const file of FILES) {
    if (!fs.existsSync(path.join(ROOT, file))) {
      diagnostics.push({ severity: 'error', file, message: 'required file is missing' });
    }
  }
  for (const [id, file, needle] of NEEDLES) {
    const full = path.join(ROOT, file);
    const text = fs.existsSync(full) ? fs.readFileSync(full, 'utf8') : '';
    if (!text.includes(needle)) {
      diagnostics.push({ severity: 'error', id, file, message: `missing needle: ${needle}` });
    }
  }
  const ok = diagnostics.length === 0;
  const result = {
    ok,
    checker: 'check-v3-shared-memory-isomorphism',
    files: FILES.length,
    needles: NEEDLES.length,
    diagnostics,
  };
  if (opts.json) console.log(JSON.stringify(result, null, 2));
  else if (ok) console.log(`v3 shared-memory-runtime Lisp/code isomorphism check OK (${FILES.length} files, ${NEEDLES.length} needles)`);
  else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 shared-memory-runtime Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(ok ? 0 : 1);
}

main();
