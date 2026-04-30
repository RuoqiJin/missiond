#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-mission-execution-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 mission_execution Lisp/code isomorphism contract. The large
agent_execution.rs runtime is deliberately split into three V3 surfaces:
  - mission_execution-log: companion log, action routing, event projection
  - agent_execution/log_governance.rs: deviation/decision/issue governance
    records plus matching live event projection
  - agent_execution/log_status.rs: status/read-model projection for companion
    logs, claims, issues, decisions, and completion durability
  - agent_execution/log_counters.rs: id-counters allocation, repair insertion,
    and legacy ID scanning shared by claim/governance/completion surfaces
  - agent_execution/lisp_syntax.rs: shared S-expression parser and delimiter
    checker used by all Lisp-backed mission_execution surfaces
  - agent_execution/session_trace.rs: optional task session-trace projection
    used by the mission_execution-log and completion-audit surfaces
  - mission_execution-claim-lease: claim/heartbeat/release and scope conflict rules
  - mission_execution-completion-audit: completion metadata, scoped commit audit,
    task contract verification, auto-verifier, repair, and audit
  - agent_execution/completion_records.rs: completion record parser, status enums,
    and durability projections shared by log/status/audit/preflight surfaces
  - agent_execution/completion_maintenance.rs: read-only audit, repair, stale-claim
    events, and derived-index rebuilds for the completion-audit surface
  - agent_execution/completion_gates.rs: scoped-commit and task-contract
    completion enforcement gates used by the completion-audit surface
  - agent_execution/completion_trace.rs: opt-in session-trace projection for
    completion records
  - agent_execution/completion_verification.rs: completion verification-source
    decision layer for daemon auto-verifier and legacy verified claims
  - agent_execution/preflight.rs: read-only pre-commit git/status and task-contract
    action wiring used by the completion-audit surface
  - agent_execution/preflight_scope.rs: porcelain parsing, claim-scope projection,
    contract scope projection, and read-only git status for preflight
  - agent_execution/task_verifier_inputs.rs: report-contract, task-contract,
    and shared-memory Lisp input projectors for verifier gates
  - agent_execution/task_verifier_auto.rs: in-process task-run verifier
    projection over task-contract/report/shared-memory artifacts
  - agent_execution/task_verifier.rs: read-only verifier gate orchestration
    used by the completion-audit surface
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  daemon: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs',
  tests: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/tests.rs',
  logSurface: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs',
  logGovernance:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_governance.rs',
  logStatus: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_status.rs',
  logCounters:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_counters.rs',
  logStore: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_store.rs',
  lispSyntax: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/lisp_syntax.rs',
  sessionTrace:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/session_trace.rs',
  claimLease: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs',
  completionAudit:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs',
  completionEntry:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_entry.rs',
  completionMaintenance:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_maintenance.rs',
  completionRecords:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_records.rs',
  completionResponse:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_response.rs',
  completionGates:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_gates.rs',
  completionTrace:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_trace.rs',
  completionVerification:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_verification.rs',
  preflight: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight.rs',
  preflightPatterns:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_patterns.rs',
  preflightPorcelain:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_porcelain.rs',
  preflightScope:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_scope.rs',
  taskVerifier:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier.rs',
  taskVerifierAuto:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_auto.rs',
  taskVerifierInputs:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_inputs.rs',
  mcp: 'crates/missiond-mcp/src/tools/knowledge/agent_execution.rs',
};

const AGGREGATE_COMMAND = 'node scripts/check-v3-mission-execution-isomorphism.mjs';

const SURFACES = [
  {
    name: 'mission_execution-log',
    noteNeedles: ['agent_execution/log_store.rs', 'agent_execution/log_counters.rs', 'agent_execution/log_governance.rs', 'agent_execution/log_status.rs', 'agent_execution/lisp_syntax.rs', 'emit_execution_event', 'agent_execution/session_trace.rs'],
  },
  {
    name: 'mission_execution-claim-lease',
    noteNeedles: ['DEFAULT_LEASE_SECS', 'scopes_overlap_pure', 'action_claim', 'action_heartbeat', 'action_release'],
  },
  {
    name: 'mission_execution-completion-audit',
    noteNeedles: ['agent_execution/completion_records.rs', 'agent_execution/completion_entry.rs', 'agent_execution/completion_response.rs', 'agent_execution/completion_maintenance.rs', 'VALID_COMMIT_STATUSES', 'agent_execution/completion_gates.rs', 'agent_execution/completion_trace.rs', 'agent_execution/completion_verification.rs', 'agent_execution/task_verifier.rs', 'agent_execution/task_verifier_auto.rs', 'agent_execution/task_verifier_inputs.rs', 'agent_execution/preflight.rs', 'agent_execution/preflight_patterns.rs', 'agent_execution/preflight_porcelain.rs'],
  },
];

const BLUEPRINT_NEEDLES = [
  ':status "code-aligned"',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/tests.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_governance.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_status.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_counters.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_store.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/lisp_syntax.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/session_trace.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_entry.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_maintenance.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_records.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_response.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_gates.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_trace.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_verification.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_patterns.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_porcelain.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_scope.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_auto.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_inputs.rs',
  'crates/missiond-mcp/src/tools/knowledge/agent_execution.rs',
  AGGREGATE_COMMAND,
];

const DAEMON_NEEDLES = [
  'pub(crate) async fn handle',
  '"open" => action_open',
  '"list" => action_list',
  '"claim" => action_claim',
  '"heartbeat" => action_heartbeat',
  '"release" => action_release',
  '"deviate" => action_deviate',
  '"decide" => action_decide',
  '"issue" => action_issue',
  '"complete" => action_complete',
  '"status" => action_status',
  '"audit" => action_audit',
  '"repair" => action_repair',
  '"preflight_commit" => action_preflight_commit',
  'mod log_store',
  'mod lisp_syntax',
  'mod log_counters',
  'mod log_governance',
  'mod log_status',
  'mod log_surface',
  'mod session_trace',
  'mod claim_lease',
  'mod completion_audit',
  'mod completion_entry',
  'mod completion_maintenance',
  'mod completion_records',
  'mod completion_response',
  'mod completion_gates',
  'mod completion_trace',
  'mod completion_verification',
  'mod preflight',
  'mod preflight_patterns',
  'mod preflight_porcelain',
  'mod preflight_scope',
  'mod task_verifier',
  'mod task_verifier_auto',
  'mod task_verifier_inputs',
  '#[cfg(test)]',
  'mod tests;',
  'use self::log_store::{',
  'use self::log_counters',
  'use self::log_governance',
  'use self::log_status::action_status',
  'use self::lisp_syntax',
  'use self::log_surface::{',
  'use self::session_trace',
  'pub(super) use self::claim_lease::scopes_overlap_pure',
  'use self::completion_audit',
  'use self::completion_maintenance',
  'use self::completion_records',
  'use self::preflight::action_preflight_commit',
  'use self::preflight_patterns',
  'use self::preflight_porcelain',
  'use self::preflight_scope',
  'use self::task_verifier',
  'use self::task_verifier_auto',
  'use self::task_verifier_inputs',
];

const TESTS_NEEDLES = [
  'use super::*;',
  'fn template_parses_and_balances',
  'fn complete_writes_each_commit_status_value',
  'fn audit_flags_scoped_commit_violation',
  'fn resolve_trace_task_id_falls_back_to_execution_id',
];

const LOG_STORE_NEEDLES = [
  'pub(super) const COMPANION_DIR: &str = ".missiond/v2"',
  'pub(super) async fn resolve_project_root',
  'pub(super) fn companion_path',
  'pub(super) fn project_or_target_project',
  'pub(super) fn require_str',
  'pub(super) struct LogFile',
  'pub(super) fn now_iso',
  'pub(super) fn parse_kv_pairs',
  'pub(super) fn lisp_quote_string',
  'pub(super) fn render_canonical_template',
  'pub(super) fn locate_kv_value',
  'pub(super) fn update_kv_in_node',
  'pub(super) fn list_block_summaries',
  'pub(super) fn json_strip_quotes',
  'pub(super) fn append_to_block',
  'pub(super) fn touch_last_updated',
  'pub(super) fn write_log_file',
  'pub(super) fn read_log_file',
];

const LOG_COUNTERS_NEEDLES = [
  'pub(super) enum Counter',
  'pub(super) fn key',
  'pub(super) fn prefix',
  'pub(super) fn block_name',
  'pub(super) fn insert_id_counters_block',
  'pub(super) fn allocate_id',
  'pub(super) fn scan_max_id',
  'locate_kv_value',
  'NodeKind::Str',
  'NodeKind::Atom',
];

const LISP_SYNTAX_NEEDLES = [
  'pub struct Node',
  'pub enum NodeKind',
  'pub fn parse',
  'fn read_form',
  'fn read_list',
  'fn read_string',
  'fn read_atom',
  'fn skip_ws_and_comments',
  'pub fn check_balance',
];

const LOG_SURFACE_NEEDLES = [
  'const VALID_DISPATCH_STRATEGIES',
  'pub(super) const DEFAULT_DISPATCH_STRATEGY',
  'pub(super) fn normalize_dispatch_strategy',
  'pub(super) async fn action_open',
  'pub(super) async fn action_list',
  'pub(super) async fn emit_execution_event',
  'pub(super) fn build_opened_event',
  'pub(super) struct DispatchMeta',
  'pub(super) fn read_dispatch_metadata_from_log',
];

const LOG_GOVERNANCE_NEEDLES = [
  'pub(super) async fn action_deviate',
  'pub(super) async fn action_decide',
  'pub(super) async fn action_issue',
  'ExecutionEvent::DeviationRecorded',
  'ExecutionEvent::DecisionRecorded',
  'ExecutionEvent::IssueRecorded',
  'read_dispatch_metadata_from_log',
  'append_to_block',
];

const LOG_STATUS_NEEDLES = [
  'pub(super) async fn action_status',
  'parse_claims',
  'parse_completions',
  'summarize_durability',
  'list_block_summaries',
  'json_strip_quotes',
  'completed_phases',
];

const SESSION_TRACE_NEEDLES = [
  'const TRACE_ID_RE',
  'pub(super) enum TraceKind',
  'pub(super) struct TraceEvent',
  'pub(super) enum TraceWarning',
  'pub(super) fn is_valid_trace_id',
  'pub(super) fn append_session_trace_event',
  'pub(super) fn resolve_session_trace_path',
  'pub(super) fn resolve_trace_task_id',
  'pub(super) fn sanitize_trace_backend',
  'pub(super) fn render_trace_event',
  'pub(super) fn scan_max_trace_seq',
];

const CLAIM_LEASE_NEEDLES = [
  'pub(super) const DEFAULT_LEASE_SECS: i64 = 1800',
  'pub(super) const MAX_LEASE_SECS: i64 = 24 * 3600',
  'pub(super) fn scopes_overlap',
  'pub(in crate::handlers::knowledge) fn scopes_overlap_pure',
  'pub(super) struct ClaimRecord',
  'pub(super) fn parse_claims',
  'pub(super) fn parse_iso',
  'pub(super) fn find_claim_node',
  'pub(super) async fn action_claim',
  'pub(super) async fn action_heartbeat',
  'pub(super) async fn action_release',
];

const COMPLETION_AUDIT_NEEDLES = [
  'pub(super) async fn action_complete',
];

const COMPLETION_ENTRY_NEEDLES = [
  'pub(super) struct CompletionEntryFields',
  'pub(super) fn render_completion_entry',
  ':changed-files',
  ':task-contract-path',
  ':task-run-verifier-status',
  ':verified',
  'render_string_list',
  'lisp_quote_string',
];

const COMPLETION_MAINTENANCE_NEEDLES = [
  'pub(super) async fn action_audit',
  'pub(super) async fn action_repair',
  'fn rebuild_derived_indexes',
  'ExecutionEvent::Audited',
  'ExecutionEvent::StaleClaim',
  'ExecutionEvent::Repaired',
];

const COMPLETION_RECORDS_NEEDLES = [
  'const VALID_COMMIT_STATUSES',
  'const VALID_VERIFIER_STATUSES',
  'const VALID_TASK_RUN_VERIFIER_STATUSES',
  'const FINDING_SCOPED_COMMIT_VIOLATION',
  'pub(super) fn normalize_commit_status',
  'pub(super) fn normalize_verifier_status',
  'pub(super) fn normalize_task_run_verifier_status',
  'pub(super) fn collect_string_list',
  'pub(super) fn render_string_list',
  'pub(super) struct CompletionRecord',
  'pub(super) fn parse_completions',
  'pub(super) fn summarize_durability',
  'pub(super) fn parse_string_list',
];

const COMPLETION_RESPONSE_NEEDLES = [
  'pub(super) struct CompletionResponseFields',
  'pub(super) fn build_completion_response',
  'verification_source',
  'verified_scope_summary',
  'verified_validation',
  'scoped_commit_validation',
  'task_contract_validation',
  'CompletionVerificationOutcome',
];

const COMPLETION_GATES_NEEDLES = [
  'pub(super) fn check_id_monotonic',
  'pub(super) fn audit_scoped_commit_handoff',
  'pub(super) fn enforce_scoped_commit_completion',
  'pub(super) fn enforce_task_contract_completion',
  'COMMIT_HASH_REQUIRED',
  'SCOPED_COMMIT_VIOLATION',
  'TASK_CONTRACT_MALFORMED',
  'CLAIM_SCOPE_MISSING',
];

const COMPLETION_TRACE_NEEDLES = [
  'pub(super) fn append_completion_trace_if_requested',
  'resolve_session_trace_path',
  'resolve_trace_task_id',
  'TraceKind::Failure',
  'TraceKind::Complete',
  'append_session_trace_event',
  'trace_warning',
];

const COMPLETION_VERIFICATION_NEEDLES = [
  'pub(super) struct CompletionVerificationOutcome',
  'pub(super) fn evaluate_completion_verification',
  'daemon-auto-verifier',
  'legacy-caller-claim',
  'auto_run_task_run_verifier',
  'task_contract_path',
  'shared_memory_path',
];

const TASK_VERIFIER_NEEDLES = [
  'pub(super) fn enforce_verified_completion',
  'read_report_summary',
  'read_task_contract_id',
];

const TASK_VERIFIER_AUTO_NEEDLES = [
  'pub(super) fn auto_run_task_run_verifier',
  'TASK_REPORT_COMMIT_HASH_MISMATCH',
  'SHARED_MEMORY_NO_COMPLETION_FOR_TASK',
  'read_report_summary',
  'read_shared_memory_ledger',
  'read_task_contract_id',
];

const TASK_VERIFIER_INPUTS_NEEDLES = [
  'pub(super) struct ReportSummary',
  'pub(super) fn read_report_summary',
  'pub(super) fn read_task_contract_id',
  'pub(super) struct SharedMemorySummary',
  'pub(super) fn read_shared_memory_ledger',
  'pub(super) fn read_completion_task_id',
  '(shared-memory ...)',
];

const PREFLIGHT_NEEDLES = [
  'pub(super) async fn action_preflight_commit',
  'resolve_session_trace_path',
];

const PREFLIGHT_PORCELAIN_NEEDLES = [
  'pub(super) struct PorcelainEntry',
  'pub(super) fn parse_porcelain_status',
  'pub(super) fn run_git_status',
  'Command::new("git")',
  '.args(["status", "--porcelain=v1"])',
];

const PREFLIGHT_PATTERNS_NEEDLES = [
  'pub(super) fn pattern_matches_path',
  'fn normalize_repo_relative',
  'fn glob_to_regex',
  'regex::Regex',
];

const PREFLIGHT_SCOPE_NEEDLES = [
  'pub(super) fn collect_all_claim_scopes',
  'pub(super) fn collect_specific_claim_scope',
  'pub(super) fn build_contract_scope_summary',
  'pub(super) fn evaluate_task_contract_for_preflight',
  'pub(super) fn build_preflight_summary',
];

const MCP_NEEDLES = [
  'ToolDefinition::new(',
  '"mission_execution"',
  '"open"',
  '"claim"',
  '"heartbeat"',
  '"release"',
  '"complete"',
  '"audit"',
  '"repair"',
  '"preflight_commit"',
  '"dispatch_strategy"',
  '"requested_cwd"',
  '"lease_secs"',
  '"commit_status"',
  '"verifier_status"',
  '"task_run_verifier_status"',
  '"task_contract_path"',
  '"task_report_path"',
  '"shared_memory_path"',
  '"session_trace_path"',
];

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
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
  if (dryFixture) {
    runFixtures(json);
    return;
  }

  const diagnostics = checkFiles(process.cwd(), DEFAULT_FILES);
  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 mission_execution Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 mission_execution Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    try {
      sources[key] = fs.readFileSync(path.join(root, rel), 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  for (const surface of SURFACES) {
    requireAll(diagnostics, files.blueprint, sources.blueprint, [`(surface ${surface.name}`]);
    requireSurfaceNoteContains(diagnostics, files.blueprint, sources.blueprint, surface.name, surface.noteNeedles);
  }
  requireAll(diagnostics, files.blueprint, sources.blueprint, BLUEPRINT_NEEDLES);
  requireAll(diagnostics, files.daemon, sources.daemon, DAEMON_NEEDLES);
  requireAll(diagnostics, files.tests, sources.tests, TESTS_NEEDLES);
  requireAll(diagnostics, files.logStore, sources.logStore, LOG_STORE_NEEDLES);
  requireAll(diagnostics, files.logCounters, sources.logCounters, LOG_COUNTERS_NEEDLES);
  requireAll(diagnostics, files.lispSyntax, sources.lispSyntax, LISP_SYNTAX_NEEDLES);
  requireAll(diagnostics, files.logSurface, sources.logSurface, LOG_SURFACE_NEEDLES);
  requireAll(diagnostics, files.logGovernance, sources.logGovernance, LOG_GOVERNANCE_NEEDLES);
  requireAll(diagnostics, files.logStatus, sources.logStatus, LOG_STATUS_NEEDLES);
  requireAll(diagnostics, files.sessionTrace, sources.sessionTrace, SESSION_TRACE_NEEDLES);
  requireAll(diagnostics, files.claimLease, sources.claimLease, CLAIM_LEASE_NEEDLES);
  requireAll(
    diagnostics,
    files.completionAudit,
    sources.completionAudit,
    COMPLETION_AUDIT_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.completionEntry,
    sources.completionEntry,
    COMPLETION_ENTRY_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.completionMaintenance,
    sources.completionMaintenance,
    COMPLETION_MAINTENANCE_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.completionRecords,
    sources.completionRecords,
    COMPLETION_RECORDS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.completionResponse,
    sources.completionResponse,
    COMPLETION_RESPONSE_NEEDLES,
  );
  requireAll(diagnostics, files.completionGates, sources.completionGates, COMPLETION_GATES_NEEDLES);
  requireAll(diagnostics, files.completionTrace, sources.completionTrace, COMPLETION_TRACE_NEEDLES);
  requireAll(
    diagnostics,
    files.completionVerification,
    sources.completionVerification,
    COMPLETION_VERIFICATION_NEEDLES,
  );
  requireAll(diagnostics, files.preflight, sources.preflight, PREFLIGHT_NEEDLES);
  requireAll(
    diagnostics,
    files.preflightPatterns,
    sources.preflightPatterns,
    PREFLIGHT_PATTERNS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.preflightPorcelain,
    sources.preflightPorcelain,
    PREFLIGHT_PORCELAIN_NEEDLES,
  );
  requireAll(diagnostics, files.preflightScope, sources.preflightScope, PREFLIGHT_SCOPE_NEEDLES);
  requireAll(diagnostics, files.taskVerifier, sources.taskVerifier, TASK_VERIFIER_NEEDLES);
  requireAll(
    diagnostics,
    files.taskVerifierAuto,
    sources.taskVerifierAuto,
    TASK_VERIFIER_AUTO_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.taskVerifierInputs,
    sources.taskVerifierInputs,
    TASK_VERIFIER_INPUTS_NEEDLES,
  );
  requireAll(diagnostics, files.mcp, sources.mcp, MCP_NEEDLES);
  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function requireSurfaceNoteContains(diagnostics, file, source, surfaceName, needles) {
  const start = source.indexOf(`(surface ${surfaceName}`);
  if (start < 0) return;
  const rest = source.slice(start + 1);
  const nextRelative = rest.search(/\n\s*\(surface\s+/);
  const body = nextRelative >= 0 ? source.slice(start, start + 1 + nextRelative) : source.slice(start);
  for (const needle of needles) {
    if (!body.includes(needle)) {
      diagnostics.push({
        file,
        message: `(surface ${surfaceName} ...) note missing required anchor: ${needle}`,
      });
    }
  }
}

function runFixtures(json) {
  const goodFiles = {
    [DEFAULT_FILES.blueprint]: buildGoodBlueprint(),
    [DEFAULT_FILES.daemon]: buildGoodDaemon(),
    [DEFAULT_FILES.tests]: buildGoodTests(),
    [DEFAULT_FILES.logStore]: buildGoodLogStore(),
    [DEFAULT_FILES.logCounters]: buildGoodLogCounters(),
    [DEFAULT_FILES.lispSyntax]: buildGoodLispSyntax(),
    [DEFAULT_FILES.logSurface]: buildGoodLogSurface(),
    [DEFAULT_FILES.logGovernance]: buildGoodLogGovernance(),
    [DEFAULT_FILES.logStatus]: buildGoodLogStatus(),
    [DEFAULT_FILES.sessionTrace]: buildGoodSessionTrace(),
    [DEFAULT_FILES.claimLease]: buildGoodClaimLease(),
    [DEFAULT_FILES.completionAudit]: buildGoodCompletionAudit(),
    [DEFAULT_FILES.completionEntry]: buildGoodCompletionEntry(),
    [DEFAULT_FILES.completionMaintenance]: buildGoodCompletionMaintenance(),
    [DEFAULT_FILES.completionRecords]: buildGoodCompletionRecords(),
    [DEFAULT_FILES.completionResponse]: buildGoodCompletionResponse(),
    [DEFAULT_FILES.completionGates]: buildGoodCompletionGates(),
    [DEFAULT_FILES.completionTrace]: buildGoodCompletionTrace(),
    [DEFAULT_FILES.completionVerification]: buildGoodCompletionVerification(),
    [DEFAULT_FILES.preflight]: buildGoodPreflight(),
    [DEFAULT_FILES.preflightPatterns]: buildGoodPreflightPatterns(),
    [DEFAULT_FILES.preflightPorcelain]: buildGoodPreflightPorcelain(),
    [DEFAULT_FILES.preflightScope]: buildGoodPreflightScope(),
    [DEFAULT_FILES.taskVerifier]: buildGoodTaskVerifier(),
    [DEFAULT_FILES.taskVerifierAuto]: buildGoodTaskVerifierAuto(),
    [DEFAULT_FILES.taskVerifierInputs]: buildGoodTaskVerifierInputs(),
    [DEFAULT_FILES.mcp]: buildGoodMcp(),
  };
  const cases = [
    {
      name: 'pass: split mission_execution surfaces align with daemon + MCP',
      expectOk: true,
      files: goodFiles,
    },
    {
      name: 'fail: missing claim-lease surface',
      expectOk: false,
      expectMessage: /mission_execution-claim-lease/,
      files: {
        ...goodFiles,
        [DEFAULT_FILES.blueprint]: goodFiles[DEFAULT_FILES.blueprint].replace(
          '(surface mission_execution-claim-lease',
          '(surface mission_execution-claim-GHOST',
        ),
      },
    },
    {
      name: 'fail: companion log note loses session-trace module anchor',
      expectOk: false,
      expectMessage: /agent_execution\/session_trace\.rs/,
      files: {
        ...goodFiles,
        [DEFAULT_FILES.blueprint]: goodFiles[DEFAULT_FILES.blueprint].replace(
          'agent_execution/session_trace.rs',
          'agent_execution/session_trace_GHOST.rs',
        ),
      },
    },
    {
      name: 'fail: daemon drops preflight action',
      expectOk: false,
      expectMessage: /action_preflight_commit/,
      files: {
        ...goodFiles,
        [DEFAULT_FILES.daemon]: goodFiles[DEFAULT_FILES.daemon].replace(
          '"preflight_commit" => action_preflight_commit',
          '"preflight_commit" => action_preflight_GHOST',
        ),
      },
    },
    {
      name: 'fail: MCP schema loses session trace field',
      expectOk: false,
      expectMessage: /session_trace_path/,
      files: {
        ...goodFiles,
        [DEFAULT_FILES.mcp]: goodFiles[DEFAULT_FILES.mcp].replace(
          '"session_trace_path"',
          '"session_trace_GHOST"',
        ),
      },
    },
  ];

  let failed = 0;
  for (const c of cases) {
    const root = materializeFixture(c.files);
    try {
      const diagnostics = checkFiles(root, DEFAULT_FILES);
      const ok = diagnostics.length === 0;
      if (ok !== c.expectOk) {
        failed += 1;
        console.error(`fixture FAILED: ${c.name}: expected ok=${c.expectOk}, got ok=${ok}`);
        for (const d of diagnostics) console.error(`  ${d.file}: ${d.message}`);
        continue;
      }
      if (c.expectMessage) {
        const messages = diagnostics.map((d) => d.message).join(' | ');
        if (!c.expectMessage.test(messages)) {
          failed += 1;
          console.error(`fixture FAILED: ${c.name}: expected ${c.expectMessage}, got ${messages || '(none)'}`);
        }
      }
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  }

  if (failed > 0) {
    console.error(`v3 mission_execution fixtures FAILED -- ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 mission_execution fixtures OK (${cases.length} cases)`);
  }
}

function materializeFixture(filesByPath) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-mission-execution-'));
  for (const [rel, body] of Object.entries(filesByPath)) {
    const abs = path.join(root, rel);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, body);
  }
  return root;
}

function buildGoodBlueprint() {
  return `(missiond-blueprint
  (implementation-map
    (surface mission_execution-log
	      :status "code-aligned"
	      :code ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/tests.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_governance.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_status.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_counters.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_store.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/lisp_syntax.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/session_trace.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
	      :note "agent_execution/lisp_syntax.rs owns the shared S-expression parser and check_balance delimiter audit; agent_execution/log_store.rs keeps COMPANION_DIR .missiond/v2, LogFile, companion paths, canonical template, key/value mutation, block append, and Lisp read/write helpers authoritative; agent_execution/log_counters.rs owns ID counters, allocate_id, scan_max_id, and insert_id_counters_block; action routing, emit_execution_event, and DispatchMeta stay in agent_execution/log_surface.rs; agent_execution/log_governance.rs owns action_deviate, action_decide, action_issue, and their DeviationRecorded/DecisionRecorded/IssueRecorded live event projection; agent_execution/log_status.rs owns action_status, active claims, open issues, unresolved deviations, decisions, completed_phases, and durability read-model projection; agent_execution/session_trace.rs keeps optional task traces aligned.")
	    (surface mission_execution-claim-lease
	      :status "code-aligned"
	      :code ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/tests.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
	      :note "DEFAULT_LEASE_SECS and MAX_LEASE_SECS bound action_claim and action_heartbeat; action_release closes claims; scopes_overlap_pure is the shared conflict predicate.")
	    (surface mission_execution-completion-audit
	      :status "code-aligned"
	      :code ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/tests.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_entry.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_maintenance.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_records.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_response.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_gates.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_trace.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_verification.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_porcelain.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_scope.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_auto.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_inputs.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_patterns.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
      :note "agent_execution/completion_records.rs owns VALID_COMMIT_STATUSES, verifier status enums, CompletionRecord, parse_completions, summarize_durability, collect_string_list, render_string_list, and parse_string_list; agent_execution/completion_audit.rs owns action_complete; agent_execution/completion_entry.rs owns CompletionEntryFields and render_completion_entry for companion-log Lisp entry projection; agent_execution/completion_response.rs owns CompletionResponseFields and build_completion_response for JSON egress projection; agent_execution/completion_maintenance.rs owns action_audit, action_repair, rebuild_derived_indexes, ExecutionEvent::Audited, ExecutionEvent::StaleClaim, and ExecutionEvent::Repaired; agent_execution/completion_gates.rs owns enforce_scoped_commit_completion and enforce_task_contract_completion; agent_execution/completion_trace.rs owns append_completion_trace_if_requested and the complete/failure session-trace projection; agent_execution/completion_verification.rs owns CompletionVerificationOutcome and evaluate_completion_verification for daemon-auto-verifier versus legacy-caller-claim decisioning; agent_execution/task_verifier_inputs.rs owns ReportSummary, SharedMemorySummary, read_report_summary, read_task_contract_id, read_shared_memory_ledger, and read_completion_task_id; agent_execution/task_verifier_auto.rs owns auto_run_task_run_verifier for the in-process task-run verifier over task-contract/report/shared-memory artifacts; agent_execution/task_verifier.rs owns enforce_verified_completion for the legacy verified=true gate; agent_execution/preflight.rs owns preflight_commit action wiring and session-trace observation before a writer commits; agent_execution/preflight_patterns.rs owns pattern_matches_path plus repo-relative glob normalization; agent_execution/preflight_porcelain.rs owns PorcelainEntry, parse_porcelain_status, and read-only git status; agent_execution/preflight_scope.rs owns build_preflight_summary, claim-scope projection, and task-contract scope projection."))
  (compression-contract
    :checks ["${AGGREGATE_COMMAND}"]))`;
}

function buildGoodDaemon() {
  return `pub(crate) async fn handle() {
  match action.as_str() {
    "open" => action_open,
    "list" => action_list,
    "claim" => action_claim,
    "heartbeat" => action_heartbeat,
    "release" => action_release,
    "deviate" => action_deviate,
    "decide" => action_decide,
    "issue" => action_issue,
    "complete" => action_complete,
    "status" => action_status,
    "audit" => action_audit,
    "repair" => action_repair,
    "preflight_commit" => action_preflight_commit,
  }
}
mod log_store;
mod log_counters;
mod lisp_syntax;
mod log_governance;
mod log_status;
mod log_surface;
mod session_trace;
mod claim_lease;
mod completion_audit;
mod completion_entry;
mod completion_maintenance;
mod completion_records;
mod completion_response;
mod completion_gates;
mod completion_trace;
mod completion_verification;
mod preflight;
mod preflight_patterns;
mod preflight_porcelain;
mod preflight_scope;
mod task_verifier;
mod task_verifier_auto;
mod task_verifier_inputs;
#[cfg(test)]
mod tests;
use self::log_surface::{
  build_opened_event, emit_execution_event, normalize_dispatch_strategy,
  read_dispatch_metadata_from_log,
};
use self::log_counters::{Counter};
use self::log_store::{LogFile};
use self::log_governance::{action_decide, action_deviate, action_issue};
use self::log_status::action_status;
use self::lisp_syntax as sexp;
use self::session_trace::{append_session_trace_event};
pub(super) use self::claim_lease::scopes_overlap_pure;
use self::completion_audit::{action_complete};
use self::completion_entry::{};
use self::completion_maintenance::{action_audit, action_repair};
use self::completion_records::{};
use self::completion_response::{};
use self::completion_trace::{};
use self::completion_verification::{};
use self::preflight::action_preflight_commit;
use self::preflight_patterns::{};
use self::preflight_porcelain::{};
use self::preflight_scope::{};
use self::task_verifier::{};
use self::task_verifier_auto::{};
use self::task_verifier_inputs::{};
`;
}

function buildGoodTests() {
  return `use super::*;
fn template_parses_and_balances() {}
fn complete_writes_each_commit_status_value() {}
fn audit_flags_scoped_commit_violation() {}
fn resolve_trace_task_id_falls_back_to_execution_id() {}
`;
}

function buildGoodLogStore() {
  return `pub(super) const COMPANION_DIR: &str = ".missiond/v2";
pub(super) async fn resolve_project_root() {}
pub(super) fn companion_path() {}
pub(super) fn project_or_target_project() {}
pub(super) fn require_str() {}
pub(super) struct LogFile {}
pub(super) fn now_iso() {}
pub(super) fn parse_kv_pairs() {}
pub(super) fn lisp_quote_string() {}
pub(super) fn render_canonical_template() {}
pub(super) fn locate_kv_value() {}
pub(super) fn update_kv_in_node() {}
pub(super) fn list_block_summaries() {}
pub(super) fn json_strip_quotes() {}
pub(super) fn append_to_block() {}
pub(super) fn touch_last_updated() {}
pub(super) fn write_log_file() {}
pub(super) fn read_log_file() {}
`;
}

function buildGoodLogCounters() {
  return `pub(super) enum Counter {}
impl Counter {
pub(super) fn key() {}
pub(super) fn prefix() {}
pub(super) fn block_name() {}
}
pub(super) fn insert_id_counters_block() {}
pub(super) fn allocate_id() {
locate_kv_value();
}
pub(super) fn scan_max_id() {
NodeKind::Str;
NodeKind::Atom;
}
`;
}

function buildGoodLispSyntax() {
  return `pub struct Node {}
pub enum NodeKind {}
pub fn parse() {}
fn read_form() {}
fn read_list() {}
fn read_string() {}
fn read_atom() {}
fn skip_ws_and_comments() {}
pub fn check_balance() {}
`;
}

function buildGoodLogSurface() {
  return `const VALID_DISPATCH_STRATEGIES: &[&str] = &[];
pub(super) const DEFAULT_DISPATCH_STRATEGY: &str = "unknown";
pub(super) fn normalize_dispatch_strategy() {}
pub(super) async fn action_open() {}
pub(super) async fn action_list() {}
pub(super) async fn emit_execution_event() {}
pub(super) fn build_opened_event() {}
pub(super) struct DispatchMeta {}
pub(super) fn read_dispatch_metadata_from_log() {}
`;
}

function buildGoodLogGovernance() {
  return `pub(super) async fn action_deviate() {
  ExecutionEvent::DeviationRecorded;
  read_dispatch_metadata_from_log();
  append_to_block();
}
pub(super) async fn action_decide() {
  ExecutionEvent::DecisionRecorded;
  read_dispatch_metadata_from_log();
  append_to_block();
}
pub(super) async fn action_issue() {
  ExecutionEvent::IssueRecorded;
  read_dispatch_metadata_from_log();
  append_to_block();
}
`;
}

function buildGoodLogStatus() {
  return `pub(super) async fn action_status() {
  parse_claims();
  parse_completions();
  summarize_durability();
  list_block_summaries();
  json_strip_quotes();
  "completed_phases";
}
`;
}

function buildGoodSessionTrace() {
  return `const TRACE_ID_RE: &str = "^[a-z0-9][a-z0-9._-]*$";
pub(super) enum TraceKind {}
pub(super) struct TraceEvent {}
pub(super) enum TraceWarning {}
pub(super) fn is_valid_trace_id() {}
pub(super) fn append_session_trace_event() {}
pub(super) fn resolve_session_trace_path() {}
pub(super) fn resolve_trace_task_id() {}
pub(super) fn sanitize_trace_backend() {}
pub(super) fn render_trace_event() {}
pub(super) fn scan_max_trace_seq() {}
`;
}

function buildGoodClaimLease() {
  return `pub(super) const DEFAULT_LEASE_SECS: i64 = 1800;
pub(super) const MAX_LEASE_SECS: i64 = 24 * 3600;
pub(super) fn scopes_overlap() {}
pub(in crate::handlers::knowledge) fn scopes_overlap_pure() {}
pub(super) struct ClaimRecord {}
pub(super) fn parse_claims() {}
pub(super) fn parse_iso() {}
pub(super) fn find_claim_node() {}
pub(super) async fn action_claim() {}
pub(super) async fn action_heartbeat() {}
pub(super) async fn action_release() {}
`;
}

function buildGoodCompletionAudit() {
  return `pub(super) async fn action_complete() {}
`;
}

function buildGoodCompletionEntry() {
  return `pub(super) struct CompletionEntryFields {}
pub(super) fn render_completion_entry() {
  ":changed-files";
  ":task-contract-path";
  ":task-run-verifier-status";
  ":verified";
  render_string_list();
  lisp_quote_string();
}
`;
}

function buildGoodCompletionMaintenance() {
  return `pub(super) async fn action_audit() {
  ExecutionEvent::Audited;
  ExecutionEvent::StaleClaim;
}
pub(super) async fn action_repair() {
  ExecutionEvent::Repaired;
}
fn rebuild_derived_indexes() {}
`;
}

function buildGoodCompletionRecords() {
  return `const VALID_COMMIT_STATUSES: &[&str] = &[];
const VALID_VERIFIER_STATUSES: &[&str] = &[];
const VALID_TASK_RUN_VERIFIER_STATUSES: &[&str] = &[];
const FINDING_SCOPED_COMMIT_VIOLATION: &str = "scoped-commit-violation";
pub(super) fn normalize_commit_status() {}
pub(super) fn normalize_verifier_status() {}
pub(super) fn normalize_task_run_verifier_status() {}
pub(super) fn collect_string_list() {}
pub(super) fn render_string_list() {}
pub(super) struct CompletionRecord {}
pub(super) fn parse_completions() {}
pub(super) fn summarize_durability() {}
pub(super) fn parse_string_list() {}
`;
}

function buildGoodCompletionResponse() {
  return `use super::completion_verification::CompletionVerificationOutcome;
pub(super) struct CompletionResponseFields {}
pub(super) fn build_completion_response() {
  "verification_source";
  "verified_scope_summary";
  "verified_validation";
  "scoped_commit_validation";
  "task_contract_validation";
}
`;
}

function buildGoodCompletionGates() {
  return `pub(super) fn check_id_monotonic() {}
pub(super) fn audit_scoped_commit_handoff() {}
pub(super) fn enforce_scoped_commit_completion() {
  "COMMIT_HASH_REQUIRED";
  "SCOPED_COMMIT_VIOLATION";
}
pub(super) fn enforce_task_contract_completion() {
  "TASK_CONTRACT_MALFORMED";
  "CLAIM_SCOPE_MISSING";
}
`;
}

function buildGoodCompletionTrace() {
  return `pub(super) fn append_completion_trace_if_requested() {
  resolve_session_trace_path();
  resolve_trace_task_id();
  TraceKind::Failure;
  TraceKind::Complete;
  append_session_trace_event();
  "trace_warning";
}
`;
}

function buildGoodCompletionVerification() {
  return `pub(super) struct CompletionVerificationOutcome {}
pub(super) fn evaluate_completion_verification() {
  "daemon-auto-verifier";
  "legacy-caller-claim";
  auto_run_task_run_verifier();
  "task_contract_path";
  "shared_memory_path";
}
`;
}

function buildGoodPreflight() {
  return `pub(super) async fn action_preflight_commit() {}
resolve_session_trace_path();
`;
}

function buildGoodPreflightPatterns() {
  return `pub(super) fn pattern_matches_path() {}
fn normalize_repo_relative() {}
fn glob_to_regex() { regex::Regex; }
`;
}

function buildGoodPreflightPorcelain() {
  return `pub(super) struct PorcelainEntry {}
pub(super) fn parse_porcelain_status() {}
pub(super) fn run_git_status() {}
std::process::Command::new("git").args(["status", "--porcelain=v1"]);
`;
}

function buildGoodPreflightScope() {
  return `
pub(super) fn collect_all_claim_scopes() {}
pub(super) fn collect_specific_claim_scope() {}
pub(super) fn build_contract_scope_summary() {}
pub(super) fn evaluate_task_contract_for_preflight() {}
pub(super) fn build_preflight_summary() {}
`;
}

function buildGoodTaskVerifier() {
  return `pub(super) fn enforce_verified_completion() {
  read_report_summary();
  read_task_contract_id();
}
`;
}

function buildGoodTaskVerifierAuto() {
  return `pub(super) fn auto_run_task_run_verifier() {
  "TASK_REPORT_COMMIT_HASH_MISMATCH";
  "SHARED_MEMORY_NO_COMPLETION_FOR_TASK";
  read_report_summary();
  read_shared_memory_ledger();
  read_task_contract_id();
}
`;
}

function buildGoodTaskVerifierInputs() {
  return `pub(super) struct ReportSummary {}
pub(super) fn read_report_summary() {}
pub(super) fn read_task_contract_id() {}
pub(super) struct SharedMemorySummary {}
pub(super) fn read_shared_memory_ledger() {}
pub(super) fn read_completion_task_id() {}
panic!("no (shared-memory ...) form found");
`;
}

function buildGoodMcp() {
  return `fn definitions() {
  ToolDefinition::new("mission_execution", "x", schema);
  "open"; "claim"; "heartbeat"; "release"; "complete"; "audit"; "repair"; "preflight_commit";
  "dispatch_strategy"; "requested_cwd"; "lease_secs"; "commit_status"; "verifier_status";
  "task_run_verifier_status"; "task_contract_path"; "task_report_path"; "shared_memory_path";
  "session_trace_path";
}`;
}

main();
