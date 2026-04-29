#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-mission-execution-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 mission_execution Lisp/code isomorphism contract. The large
agent_execution.rs runtime is deliberately split into three V3 surfaces:
  - mission_execution-log: companion log, action routing, event projection, trace append
  - mission_execution-claim-lease: claim/heartbeat/release and scope conflict rules
  - mission_execution-completion-audit: completion metadata, scoped commit audit,
    task contract verification, auto-verifier, repair, and preflight
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  daemon: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs',
  logSurface: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs',
  claimLease: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs',
  completionAudit:
    'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs',
  mcp: 'crates/missiond-mcp/src/tools/knowledge/agent_execution.rs',
};

const AGGREGATE_COMMAND = 'node scripts/check-v3-mission-execution-isomorphism.mjs';

const SURFACES = [
  {
    name: 'mission_execution-log',
    noteNeedles: ['COMPANION_DIR', 'emit_execution_event', 'append_session_trace_event'],
  },
  {
    name: 'mission_execution-claim-lease',
    noteNeedles: ['DEFAULT_LEASE_SECS', 'scopes_overlap_pure', 'action_claim', 'action_heartbeat', 'action_release'],
  },
  {
    name: 'mission_execution-completion-audit',
    noteNeedles: ['VALID_COMMIT_STATUSES', 'enforce_scoped_commit_completion', 'auto_run_task_run_verifier', 'preflight_commit'],
  },
];

const BLUEPRINT_NEEDLES = [
  ':status "code-aligned"',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs',
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
  'const COMPANION_DIR: &str = ".missiond/v2"',
  'mod log_surface',
  'mod claim_lease',
  'mod completion_audit',
  'use self::log_surface::{',
  'pub(super) use self::claim_lease::scopes_overlap_pure',
  'use self::completion_audit',
  'async fn action_claim',
  'async fn action_heartbeat',
  'async fn action_release',
  'fn enforce_scoped_commit_completion',
  'fn enforce_task_contract_completion',
  'fn enforce_verified_completion',
  'fn auto_run_task_run_verifier',
  'fn build_preflight_summary',
  'async fn action_preflight_commit',
];

const LOG_SURFACE_NEEDLES = [
  'const VALID_DISPATCH_STRATEGIES',
  'const DEFAULT_DISPATCH_STRATEGY',
  'pub(super) mod sexp',
  'pub(super) struct LogFile',
  'pub(super) fn now_iso',
  'pub(super) fn parse_kv_pairs',
  'pub(super) fn lisp_quote_string',
  'pub(super) fn render_canonical_template',
  'pub(super) enum Counter',
  'pub(super) fn locate_kv_value',
  'pub(super) fn allocate_id',
  'pub(super) fn scan_max_id',
  'pub(super) fn append_to_block',
  'pub(super) fn touch_last_updated',
  'pub(super) fn write_log_file',
  'pub(super) fn read_log_file',
  'pub(super) fn normalize_dispatch_strategy',
  'pub(super) async fn emit_execution_event',
  'pub(super) fn build_opened_event',
  'pub(super) struct DispatchMeta',
  'pub(super) fn read_dispatch_metadata_from_log',
  'pub(super) enum TraceKind',
  'pub(super) struct TraceEvent',
  'pub(super) enum TraceWarning',
  'pub(super) fn append_session_trace_event',
  'pub(super) fn resolve_session_trace_path',
  'pub(super) fn resolve_trace_task_id',
  'pub(super) fn sanitize_trace_backend',
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
];

const COMPLETION_AUDIT_NEEDLES = [
  'const VALID_COMMIT_STATUSES',
  'const VALID_VERIFIER_STATUSES',
  'const VALID_TASK_RUN_VERIFIER_STATUSES',
  'const FINDING_SCOPED_COMMIT_VIOLATION',
  'pub(super) fn normalize_commit_status',
  'pub(super) fn normalize_verifier_status',
  'pub(super) fn normalize_task_run_verifier_status',
  'pub(super) struct CompletionRecord',
  'pub(super) fn parse_completions',
  'pub(super) fn summarize_durability',
  'pub(super) fn parse_string_list',
  'pub(super) struct ReportSummary',
  'pub(super) fn read_report_summary',
  'pub(super) fn read_task_contract_id',
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
  requireAll(diagnostics, files.logSurface, sources.logSurface, LOG_SURFACE_NEEDLES);
  requireAll(diagnostics, files.claimLease, sources.claimLease, CLAIM_LEASE_NEEDLES);
  requireAll(
    diagnostics,
    files.completionAudit,
    sources.completionAudit,
    COMPLETION_AUDIT_NEEDLES,
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
    [DEFAULT_FILES.logSurface]: buildGoodLogSurface(),
    [DEFAULT_FILES.claimLease]: buildGoodClaimLease(),
    [DEFAULT_FILES.completionAudit]: buildGoodCompletionAudit(),
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
      name: 'fail: companion log note loses trace append anchor',
      expectOk: false,
      expectMessage: /append_session_trace_event/,
      files: {
        ...goodFiles,
        [DEFAULT_FILES.blueprint]: goodFiles[DEFAULT_FILES.blueprint].replace(
          'append_session_trace_event',
          'trace_append_GHOST',
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
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
	      :note "COMPANION_DIR .missiond/v2 is the durable log root; action routing, emit_execution_event, DispatchMeta, and append_session_trace_event keep log writes, live events, and optional task traces aligned.")
	    (surface mission_execution-claim-lease
	      :status "code-aligned"
	      :code ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
	      :note "DEFAULT_LEASE_SECS and MAX_LEASE_SECS bound action_claim and action_heartbeat; action_release closes claims; scopes_overlap_pure is the shared conflict predicate.")
	    (surface mission_execution-completion-audit
	      :status "code-aligned"
	      :code ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
      :note "VALID_COMMIT_STATUSES, verifier status enums, enforce_scoped_commit_completion, enforce_task_contract_completion, auto_run_task_run_verifier, repair, audit, and preflight_commit form the completion durability gate."))
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
const COMPANION_DIR: &str = ".missiond/v2";
mod log_surface;
mod claim_lease;
mod completion_audit;
use self::log_surface::{
  build_opened_event, emit_execution_event, normalize_dispatch_strategy,
  read_dispatch_metadata_from_log,
};
pub(super) use self::claim_lease::scopes_overlap_pure;
use self::completion_audit::{};
async fn action_claim() {}
async fn action_heartbeat() {}
async fn action_release() {}
fn enforce_scoped_commit_completion() {}
fn enforce_task_contract_completion() {}
fn enforce_verified_completion() {}
fn auto_run_task_run_verifier() {}
fn build_preflight_summary() {}
async fn action_preflight_commit() {}
`;
}

function buildGoodLogSurface() {
  return `const VALID_DISPATCH_STRATEGIES: &[&str] = &[];
const DEFAULT_DISPATCH_STRATEGY: &str = "unknown";
pub(super) mod sexp {}
pub(super) struct LogFile {}
pub(super) fn now_iso() {}
pub(super) fn parse_kv_pairs() {}
pub(super) fn lisp_quote_string() {}
pub(super) fn render_canonical_template() {}
pub(super) enum Counter {}
pub(super) fn locate_kv_value() {}
pub(super) fn allocate_id() {}
pub(super) fn scan_max_id() {}
pub(super) fn append_to_block() {}
pub(super) fn touch_last_updated() {}
pub(super) fn write_log_file() {}
pub(super) fn read_log_file() {}
pub(super) fn normalize_dispatch_strategy() {}
pub(super) async fn emit_execution_event() {}
pub(super) fn build_opened_event() {}
pub(super) struct DispatchMeta {}
pub(super) fn read_dispatch_metadata_from_log() {}
pub(super) enum TraceKind {}
pub(super) struct TraceEvent {}
pub(super) enum TraceWarning {}
pub(super) fn append_session_trace_event() {}
pub(super) fn resolve_session_trace_path() {}
pub(super) fn resolve_trace_task_id() {}
pub(super) fn sanitize_trace_backend() {}
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
`;
}

function buildGoodCompletionAudit() {
  return `const VALID_COMMIT_STATUSES: &[&str] = &[];
const VALID_VERIFIER_STATUSES: &[&str] = &[];
const VALID_TASK_RUN_VERIFIER_STATUSES: &[&str] = &[];
const FINDING_SCOPED_COMMIT_VIOLATION: &str = "scoped-commit-violation";
pub(super) fn normalize_commit_status() {}
pub(super) fn normalize_verifier_status() {}
pub(super) fn normalize_task_run_verifier_status() {}
pub(super) struct CompletionRecord {}
pub(super) fn parse_completions() {}
pub(super) fn summarize_durability() {}
pub(super) fn parse_string_list() {}
pub(super) struct ReportSummary {}
pub(super) fn read_report_summary() {}
pub(super) fn read_task_contract_id() {}
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
