#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

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
  handlerClaim: 'crates/missiond-daemon/src/handlers/knowledge/board/claim.rs',
  handlerCreate: 'crates/missiond-daemon/src/handlers/knowledge/board/create.rs',
  handlerDecompose: 'crates/missiond-daemon/src/handlers/knowledge/board/decompose.rs',
  handlerDelete: 'crates/missiond-daemon/src/handlers/knowledge/board/delete.rs',
  handlerEvents: 'crates/missiond-daemon/src/handlers/knowledge/board/events.rs',
  handlerNote: 'crates/missiond-daemon/src/handlers/knowledge/board/note.rs',
  handlerQuery: 'crates/missiond-daemon/src/handlers/knowledge/board/query.rs',
  handlerRetry: 'crates/missiond-daemon/src/handlers/knowledge/board/retry.rs',
  handlerSession: 'crates/missiond-daemon/src/handlers/knowledge/board/session.rs',
  handlerUpdate: 'crates/missiond-daemon/src/handlers/knowledge/board/update.rs',
  types: 'crates/missiond-core/src/types/board.rs',
  traits: 'crates/missiond-core/src/db/traits.rs',
  pgStore: 'crates/missiond-core/src/db/pg/board.rs',
  flowEngine: 'crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs',
  sysinfraMisc: 'crates/missiond-daemon/src/handlers/sysinfra/misc.rs',
  aiops: 'crates/missiond-daemon/src/infra/aiops.rs',
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
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
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
    'crates/missiond-daemon/src/handlers/knowledge/board/claim.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/create.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/decompose.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/delete.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/events.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/note.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/query.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/retry.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/session.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/update.rs',
    'crates/missiond-core/src/types/board.rs',
    'crates/missiond-core/src/db/traits.rs',
    'crates/missiond-core/src/db/pg/board.rs',
    'crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs',
    'crates/missiond-daemon/src/handlers/sysinfra/misc.rs',
    'crates/missiond-daemon/src/infra/aiops.rs',
    'crates/missiond-mcp/src/tools/knowledge/board.rs',
    'BoardTaskStatus',
    'claim_board_task',
    'clear_dangling_dynamic_slot_assignees',
    'list_autopilot_tasks',
    'node scripts/check-v3-board-isomorphism.mjs',
    'normalize common MCP argument aliases',
    'structured ToolError codes',
    'compact note receipts',
    'validate parentId and dependsOn',
    'cap descriptions',
    'reject oversized note payloads',
    'aggregate self-heal incident tasks by dedupe_key',
    'mission_submit_phase_result rejects obviously short execution_plan artifacts before ConsultGemini2',
    'ConsultGemini2 stores review evidence but advances to Execute only after an explicit approval signal',
    'ConsultGemini1 remains advisory',
  ]);

  requireAll(diagnostics, files.handler, sources.handler, [
    'mod claim;',
    'mod create;',
    'mod decompose;',
    'mod delete;',
    'mod events;',
    'mod note;',
    'mod query;',
    'mod retry;',
    'mod session;',
    'mod update;',
    '"mission_board_query"',
    '"mission_board_create"',
    '"mission_board_update"',
    '"mission_board_claim"',
    '"mission_board_note_add"',
    '"mission_board_retry"',
    'claim::handle_claim',
    'create::handle_create',
    'query::handle_query',
    'retry::handle_retry',
    'normalize_board_args',
    'invalid_board_args',
    'board_store_error',
  ]);

  requireAll(diagnostics, files.handlerSession, sources.handlerSession, [
    'record_session_task_binding',
    'current_session_id()',
    'SessionTaskBinding',
    'bound_at: chrono::Utc::now().timestamp()',
  ]);

  requireAll(diagnostics, files.handlerEvents, sources.handlerEvents, [
    'publish_board_created',
    'publish_board_update',
    'publish_board_status_changed',
    'BoardEvent::TaskCreated',
    'BoardEvent::Updated',
    'BoardEvent::StatusChanged',
  ]);

  requireAll(diagnostics, files.handlerQuery, sources.handlerQuery, [
    'handle_query',
    'board_get',
    'BoardListArgs',
    'get_board_tasks_with_context',
    'get_board_task_with_notes',
    'list_board_tasks',
    'search_board_tasks',
    'BoardSearchInput',
    'board_summary',
    'clear_done_board_tasks',
  ]);

  requireAll(diagnostics, files.handlerCreate, sources.handlerCreate, [
    'handle_create',
    'CreateBoardTaskInput',
    'normalize_board_args',
    'invalid_board_args',
    'board_store_error',
    'create_board_task',
    'flow_template',
    'FlowContext::default',
    'publish_board_created',
  ]);

  requireAll(diagnostics, files.handlerUpdate, sources.handlerUpdate, [
    'handle_update',
    'handle_batch_update',
    'handle_toggle',
    'handle_single_update',
    'UpdateBoardTaskInput',
    'normalize_board_args',
    'invalid_status_result',
    'not_found_result',
    'toggle_board_task',
    'harvest_decisions_for_task',
    'publish_board_status_changed',
    'publish_board_update',
  ]);

  requireAll(diagnostics, files.handlerDelete, sources.handlerDelete, [
    'handle_delete',
    'delete_board_task',
    'BoardEvent::Deleted',
  ]);

  requireAll(diagnostics, files.handlerClaim, sources.handlerClaim, [
    'handle_claim',
    '.claim_board_task(task_id, executor_id, executor_type)',
    'current_session_id',
    'BoardEvent::Claimed',
    'record_session_task_binding',
  ]);

  requireAll(diagnostics, files.handlerNote, sources.handlerNote, [
    'handle_note_add',
    'BoardNoteAddArgs',
    'COMPACT_NOTE_RESPONSE_THRESHOLD_BYTES',
    'MAX_NOTE_CONTENT_BYTES',
    'note_add_response',
    'note_content_too_large_result',
    'invalid_board_args',
    'board_store_error',
    'add_board_task_note',
    'BoardEvent::NoteAdded',
    'record_session_task_binding',
  ]);

  requireAll(diagnostics, files.handlerDecompose, sources.handlerDecompose, [
    'handle_decompose',
    'DecomposeArgs',
    'mission_board_create',
    'mission_board_note_add',
    'submit_task',
    'SlotEvent::TaskDispatched',
    'add_board_task_note',
  ]);

  requireAll(diagnostics, files.handlerRetry, sources.handlerRetry, [
    'handle_retry',
    'RetryArgs',
    'retry_board_task',
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
    'ACTIVE_BOARD_SEARCH_STATUSES',
    'include_historical_results',
    'apply_active_status_filter',
    'activeFilterApplied',
    'historicalIncluded',
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
    'MAX_BOARD_TASK_DESCRIPTION_BYTES',
    'compact_board_task_description',
    'resolve_existing_board_task_id',
    'references unknown BoardTask id',
    'parentId cannot reference the task itself',
    'dependsOn cannot reference the task itself',
    'ACTIVE_BOARD_SEARCH_STATUSES',
    'active_filter_applied',
    'historical_included',
    'Default search scope excludes done/skipped historical tasks',
    "SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))",
    'FOR UPDATE',
    'DbError::ClaimConflict',
    'INSERT INTO work_leases',
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

  requireAll(diagnostics, files.flowEngine, sources.flowEngine, [
    'pub(crate) enum PlanReviewGateDecision',
    'validate_execution_plan_artifact',
    'MIN_PLAN_CHARS',
    'MIN_PLAN_LINES',
    'plan_review_gate_decision',
    'PlanReviewGateDecision::Approved',
    'PlanReviewGateDecision::NeedsChanges',
    'PlanReviewGateDecision::Ambiguous',
    'EngineeringPhase::ConsultGemini2',
    'Gemini review did not include an explicit APPROVED/LGTM/批准 signal',
    'Flow returned to Plan for revision',
    'decision_type: Some("review_gate".to_string())',
    'execution_plan_artifact_rejects_obviously_short_plans',
    'consult_gemini2_review_requires_explicit_approval',
  ]);

  requireAll(diagnostics, files.sysinfraMisc, sources.sysinfraMisc, [
    '"mission_submit_phase_result"',
    'validate_execution_plan_artifact',
    'execution_plan rejected before ConsultGemini2 review',
  ]);

  requireAll(diagnostics, files.aiops, sources.aiops, [
    'create_pty_remediation_task',
    'find_open_task_by_dedupe_key',
    'PTY remediation: duplicate incident aggregated into existing task',
    'auto_execute: Some(false)',
    'assignee: None',
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
    '"includeHistorical"',
    '"scope"',
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
             "crates/missiond-daemon/src/handlers/knowledge/board/claim.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/create.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/decompose.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/delete.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/note.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/retry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/session.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
             "crates/missiond-core/src/types/board.rs"
             "crates/missiond-core/src/db/traits.rs"
             "crates/missiond-core/src/db/pg/board.rs"
             "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
             "crates/missiond-daemon/src/infra/aiops.rs"
             "crates/missiond-mcp/src/tools/knowledge/board.rs"]
      :engineering-flow-gate ["mission_submit_phase_result rejects obviously short execution_plan artifacts before ConsultGemini2."
                              "ConsultGemini2 stores review evidence but advances to Execute only after an explicit approval signal."
                              "ConsultGemini1 remains advisory."]
      :note "BoardTaskStatus claim_board_task clear_dangling_dynamic_slot_assignees list_autopilot_tasks normalize common MCP argument aliases structured ToolError codes compact note receipts validate parentId and dependsOn cap descriptions reject oversized note payloads aggregate self-heal incident tasks by dedupe_key"))
  (compression-contract
    :checks ["node scripts/check-v3-board-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.handler, `
mod claim;
mod create;
mod decompose;
mod delete;
mod events;
mod note;
mod query;
mod retry;
mod session;
mod update;
"mission_board_query"
"mission_board_create"
"mission_board_update"
"mission_board_claim"
"mission_board_note_add"
"mission_board_retry"
claim::handle_claim
create::handle_create
query::handle_query
retry::handle_retry
normalize_board_args
invalid_board_args
board_store_error
`);

  writeFixture(root, DEFAULT_FILES.handlerSession, `
record_session_task_binding
current_session_id()
SessionTaskBinding
bound_at: chrono::Utc::now().timestamp()
`);

  writeFixture(root, DEFAULT_FILES.handlerEvents, `
publish_board_created
publish_board_update
publish_board_status_changed
BoardEvent::TaskCreated
BoardEvent::Updated
BoardEvent::StatusChanged
`);

  writeFixture(root, DEFAULT_FILES.handlerQuery, `
handle_query
board_get
BoardListArgs
get_board_tasks_with_context
get_board_task_with_notes
list_board_tasks
search_board_tasks
BoardSearchInput
board_summary
clear_done_board_tasks
`);

  writeFixture(root, DEFAULT_FILES.handlerCreate, `
handle_create
CreateBoardTaskInput
normalize_board_args
invalid_board_args
board_store_error
create_board_task
flow_template
FlowContext::default
publish_board_created
`);

  writeFixture(root, DEFAULT_FILES.handlerUpdate, `
handle_update
handle_batch_update
handle_toggle
handle_single_update
UpdateBoardTaskInput
normalize_board_args
invalid_status_result
not_found_result
toggle_board_task
harvest_decisions_for_task
publish_board_status_changed
publish_board_update
`);

  writeFixture(root, DEFAULT_FILES.handlerDelete, `
handle_delete
delete_board_task
BoardEvent::Deleted
`);

  writeFixture(root, DEFAULT_FILES.handlerClaim, `
handle_claim
.claim_board_task(task_id, executor_id, executor_type)
current_session_id
BoardEvent::Claimed
record_session_task_binding
`);

  writeFixture(root, DEFAULT_FILES.handlerNote, `
handle_note_add
BoardNoteAddArgs
COMPACT_NOTE_RESPONSE_THRESHOLD_BYTES
MAX_NOTE_CONTENT_BYTES
note_add_response
note_content_too_large_result
invalid_board_args
board_store_error
add_board_task_note
BoardEvent::NoteAdded
record_session_task_binding
`);

  writeFixture(root, DEFAULT_FILES.handlerDecompose, `
handle_decompose
DecomposeArgs
mission_board_create
mission_board_note_add
submit_task
SlotEvent::TaskDispatched
add_board_task_note
`);

  writeFixture(root, DEFAULT_FILES.handlerRetry, `
handle_retry
RetryArgs
retry_board_task
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
ACTIVE_BOARD_SEARCH_STATUSES
include_historical_results
apply_active_status_filter
activeFilterApplied
historicalIncluded
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
MAX_BOARD_TASK_DESCRIPTION_BYTES
compact_board_task_description
resolve_existing_board_task_id
references unknown BoardTask id
parentId cannot reference the task itself
dependsOn cannot reference the task itself
ACTIVE_BOARD_SEARCH_STATUSES
active_filter_applied
historical_included
Default search scope excludes done/skipped historical tasks
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

  writeFixture(root, DEFAULT_FILES.flowEngine, `
pub(crate) enum PlanReviewGateDecision
validate_execution_plan_artifact
MIN_PLAN_CHARS
MIN_PLAN_LINES
plan_review_gate_decision
PlanReviewGateDecision::Approved
PlanReviewGateDecision::NeedsChanges
PlanReviewGateDecision::Ambiguous
EngineeringPhase::ConsultGemini2
Gemini review did not include an explicit APPROVED/LGTM/批准 signal
Flow returned to Plan for revision
decision_type: Some("review_gate".to_string())
execution_plan_artifact_rejects_obviously_short_plans
consult_gemini2_review_requires_explicit_approval
`);

  writeFixture(root, DEFAULT_FILES.sysinfraMisc, `
"mission_submit_phase_result"
validate_execution_plan_artifact
execution_plan rejected before ConsultGemini2 review
`);

  writeFixture(root, DEFAULT_FILES.aiops, `
create_pty_remediation_task
find_open_task_by_dedupe_key
PTY remediation: duplicate incident aggregated into existing task
auto_execute: Some(false)
assignee: None
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
"includeHistorical"
"scope"
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
