#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-board-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 mission_board Lisp/code isomorphism contract:
  - BoardTask schema/status/claim/lease fields are pinned in core types.
  - BoardStore exposes the task CRUD, claim, lease, dependency, retry, and note API.
  - PostgreSQL claim/recovery queries keep open-only atomic claims and dynamic-slot cleanup.
  - Daemon board handler and MCP tool definitions expose the same task surface.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  handler: 'crates/missiond-daemon/src/handlers/knowledge/board.rs',
  types: 'crates/missiond-core/src/types/board.rs',
  traits: 'crates/missiond-core/src/db/traits.rs',
  pgStore: 'crates/missiond-core/src/db/pg/board.rs',
  mcp: 'crates/missiond-mcp/src/tools/knowledge/board.rs',
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
    console.log('v3 mission_board Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 mission_board Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
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
    'mission_board',
    '(surface mission_board',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/handlers/knowledge/board.rs',
    'crates/missiond-core/src/types/board.rs',
    'crates/missiond-core/src/db/traits.rs',
    'crates/missiond-core/src/db/pg/board.rs',
    'crates/missiond-mcp/src/tools/knowledge/board.rs',
    'BoardTaskStatus',
    'claim_board_task',
    'clear_dangling_dynamic_slot_assignees',
    'list_autopilot_tasks',
    'node scripts/check-v3-board-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.handler, sources.handler, [
    'record_session_task_binding',
    'publish_board_created',
    'publish_board_update',
    'publish_board_status_changed',
    '"mission_board_query"',
    '"mission_board_create"',
    '"mission_board_update"',
    '"mission_board_claim"',
    '"mission_board_note_add"',
    '"mission_board_retry"',
    '.claim_board_task(task_id, executor_id, executor_type)',
    'add_board_task_note',
  ]);

  requireAll(diagnostics, files.types, sources.types, [
    'pub enum BoardTaskStatus',
    'Open',
    'Running',
    'Verifying',
    'Done',
    'Blocked',
    'Failed',
    'Skipped',
    'pub struct BoardTask',
    'pub assignee: Option<String>',
    'pub claim_executor_id: Option<String>',
    'pub claim_executor_type: Option<String>',
    'pub claimed_at: Option<String>',
    'pub depends_on: Vec<TaskId>',
    'pub lease_expires_at: Option<String>',
    'pub timeout_secs: Option<i64>',
    'pub notes_count: i64',
  ]);

  requireAll(diagnostics, files.traits, sources.traits, [
    'pub trait BoardStore',
    'async fn create_board_task',
    'async fn update_board_task',
    'async fn claim_board_task',
    'async fn clear_board_task_assignee',
    'async fn release_board_claims_by_executor',
    'async fn recover_stale_running_tasks',
    'async fn clear_dangling_dynamic_slot_assignees',
    'async fn set_board_task_lease',
    'async fn list_autopilot_tasks',
    'async fn check_dependencies',
    'async fn retry_board_task',
    'async fn add_board_task_note',
  ]);

  requireAll(diagnostics, files.pgStore, sources.pgStore, [
    'impl BoardStore for PgMissionStore',
    "WHERE id = $4 AND status = 'open' AND claim_executor_id IS NULL",
    "WHERE id = $2 AND status = 'open' AND assignee = $3",
    "WHERE claim_executor_id = $2 AND status = 'running'",
    "WHERE status = 'running'",
    "lease_expires_at < $1",
    'COALESCE(timeout_secs, $2)',
    "WHERE assignee LIKE 'slot-dyn-%'",
    "AND status = 'active'",
    'async fn set_board_task_lease',
    "WHERE id = $3 AND status = 'running'",
    'async fn list_autopilot_tasks',
    'CASE WHEN assignee IS NOT NULL THEN 0 ELSE 1 END',
    'async fn check_dependencies',
    'async fn retry_board_task',
  ]);

  requireAll(diagnostics, files.mcp, sources.mcp, [
    'Source: .missiond/intent-tools.lisp (module board)',
    '"mission_board_query"',
    '"mission_board_create"',
    '"mission_board_update"',
    '"mission_board_delete"',
    '"mission_board_claim"',
    '"mission_board_note_add"',
    '"mission_board_decompose"',
    '"mission_board_retry"',
    '"open"',
    '"running"',
    '"done"',
    '"failed"',
    '"blocked"',
    '"taskId"',
    '"executorType"',
    '"dependsOn"',
    '"flowTemplate"',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-board-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (implementation-map
    (surface mission_board
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/knowledge/board.rs"
             "crates/missiond-core/src/types/board.rs"
             "crates/missiond-core/src/db/traits.rs"
             "crates/missiond-core/src/db/pg/board.rs"
             "crates/missiond-mcp/src/tools/knowledge/board.rs"]
      :note "BoardTaskStatus claim_board_task clear_dangling_dynamic_slot_assignees list_autopilot_tasks"))
  (compression-contract
    :checks ["node scripts/check-v3-board-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.handler, `
record_session_task_binding
publish_board_created
publish_board_update
publish_board_status_changed
"mission_board_query"
"mission_board_create"
"mission_board_update"
"mission_board_claim"
"mission_board_note_add"
"mission_board_retry"
.claim_board_task(task_id, executor_id, executor_type)
add_board_task_note
`);

  writeFixture(root, DEFAULT_FILES.types, `
pub enum BoardTaskStatus
Open
Running
Verifying
Done
Blocked
Failed
Skipped
pub struct BoardTask
pub assignee: Option<String>
pub claim_executor_id: Option<String>
pub claim_executor_type: Option<String>
pub claimed_at: Option<String>
pub depends_on: Vec<TaskId>
pub lease_expires_at: Option<String>
pub timeout_secs: Option<i64>
pub notes_count: i64
`);

  writeFixture(root, DEFAULT_FILES.traits, `
pub trait BoardStore
async fn create_board_task
async fn update_board_task
async fn claim_board_task
async fn clear_board_task_assignee
async fn release_board_claims_by_executor
async fn recover_stale_running_tasks
async fn clear_dangling_dynamic_slot_assignees
async fn set_board_task_lease
async fn list_autopilot_tasks
async fn check_dependencies
async fn retry_board_task
async fn add_board_task_note
`);

  writeFixture(root, DEFAULT_FILES.pgStore, `
impl BoardStore for PgMissionStore
WHERE id = $4 AND status = 'open' AND claim_executor_id IS NULL
WHERE id = $2 AND status = 'open' AND assignee = $3
WHERE claim_executor_id = $2 AND status = 'running'
WHERE status = 'running'
lease_expires_at < $1
COALESCE(timeout_secs, $2)
WHERE assignee LIKE 'slot-dyn-%'
AND status = 'active'
async fn set_board_task_lease
WHERE id = $3 AND status = 'running'
async fn list_autopilot_tasks
CASE WHEN assignee IS NOT NULL THEN 0 ELSE 1 END
async fn check_dependencies
async fn retry_board_task
`);

  writeFixture(root, DEFAULT_FILES.mcp, `
Source: .missiond/intent-tools.lisp (module board)
"mission_board_query"
"mission_board_create"
"mission_board_update"
"mission_board_delete"
"mission_board_claim"
"mission_board_note_add"
"mission_board_decompose"
"mission_board_retry"
"open"
"running"
"done"
"failed"
"blocked"
"taskId"
"executorType"
"dependsOn"
"flowTemplate"
`);

  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content.trimStart(), 'utf8');
}

main();
