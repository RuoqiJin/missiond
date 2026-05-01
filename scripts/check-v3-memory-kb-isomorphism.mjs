#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-memory-kb-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 memory-kb convergence contract. The surface is now
:status "code-aligned": kb.rs stays the facade while kb/* modules own
the corresponding V3 function boundaries.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  v3Runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  kbFacade: 'crates/missiond-daemon/src/handlers/knowledge/kb.rs',
  kbArgs: 'crates/missiond-daemon/src/handlers/knowledge/kb/args.rs',
  kbRemember: 'crates/missiond-daemon/src/handlers/knowledge/kb/remember.rs',
  kbQuality: 'crates/missiond-daemon/src/handlers/knowledge/kb/quality.rs',
  kbCompact: 'crates/missiond-daemon/src/handlers/knowledge/kb/compact.rs',
  kbConflicts: 'crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs',
  kbQuery: 'crates/missiond-daemon/src/handlers/knowledge/kb/query.rs',
  kbDiscovery: 'crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs',
  kbAnalyze: 'crates/missiond-daemon/src/handlers/knowledge/kb/analyze.rs',
  kbMutate: 'crates/missiond-daemon/src/handlers/knowledge/kb/mutate.rs',
  kbImport: 'crates/missiond-daemon/src/handlers/knowledge/kb/import.rs',
  kbGc: 'crates/missiond-daemon/src/handlers/knowledge/kb/gc.rs',
  kbOps: 'crates/missiond-daemon/src/handlers/knowledge/kb/ops.rs',
  kbBeacon: 'crates/missiond-daemon/src/handlers/knowledge/kb/beacon.rs',
  kbCodeSearch: 'crates/missiond-daemon/src/handlers/knowledge/kb/code_search.rs',
  memory: 'crates/missiond-daemon/src/handlers/knowledge/memory.rs',
  learningMod: 'crates/missiond-daemon/src/engine/learning_engine/mod.rs',
  learningExtraction: 'crates/missiond-daemon/src/engine/learning_engine/extraction.rs',
  learningDecision: 'crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs',
  learningTimeline: 'crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs',
  learningIdle: 'crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs',
  learningHistorical: 'crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs',
  mcpKb: 'crates/missiond-mcp/src/tools/knowledge/kb.rs',
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
    console.log('v3 memory-kb Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 memory-kb Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
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
    '(surface memory-kb',
	    '(memory-kb-policy',
	    '(learning-engine-policy',
	    ':pending-message-limit 60',
	    ':tool-result-preview-chars 1000',
	    ':assistant-preview-chars 500',
	    ':realtime-extraction-timeout-secs 300',
	    ':decision-tier3-timeout-secs 300',
	    ':timeline-analysis-interval-secs 43200',
	    ':habit-scan-timeout-secs 600',
	    'LearningEngineRuntimeConfig MUST load learning-engine-policy',
	    ':status "code-aligned"',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/args.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/remember.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/quality.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/compact.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/query.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/analyze.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/mutate.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/import.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/gc.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/ops.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/beacon.rs',
	    'crates/missiond-daemon/src/handlers/knowledge/kb/code_search.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/mod.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/extraction.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs',
	    'scripts/check-v3-memory-kb-isomorphism.mjs',
	    'memory-kb-policy realtime extraction batch size and preview truncation budgets',
	    'projects learning-engine-policy into learning_engine pty send budgets, maintenance cadences, timeline read windows, and KB reflection policy',
    'kb.rs remains the memory-kb facade',
    'kb/args.rs owns unified KB argument ingress',
    'kb/remember.rs owns remember ingestion, graph edge side effects, embedding trigger, mutation event, and conflict downweighting',
    'kb/quality.rs owns content-quality rejection',
    'kb/compact.rs owns rule-based KB compaction',
    'kb/conflicts.rs owns semantic conflict detection',
    'kb/query.rs owns search/get/list retrieval egress',
    'kb/discovery.rs owns SSH probe discovery and infra KB projection',
    'kb/analyze.rs owns LLM analysis, context-budgeting, and consolidation-plan queue projection',
    'kb/mutate.rs owns forget/update/project mutation side effects',
    'kb/import.rs owns servers_yaml import projection',
    'kb/gc.rs owns stats/stale/duplicates cleanup actions',
    'kb/ops.rs owns queue-status and execute-plan operation egress',
    'kb/beacon.rs owns unified mission_beacon action routing plus legacy beacon list/map/tag/annotate',
    'kb/code_search.rs owns AST code-search egress',
    'node scripts/check-v3-memory-kb-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'MemoryKbRuntimeConfig',
    'LearningEngineRuntimeConfig',
    'parse_memory_kb_policy',
    'parse_learning_engine_policy',
    'DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT',
    'DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS',
    'DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS',
    'memory-kb-policy',
    ':pending-message-limit',
    ':tool-result-preview-chars',
    ':assistant-preview-chars',
    'DEFAULT_LEARNING_REALTIME_EXTRACTION_TIMEOUT_SECS',
    'DEFAULT_LEARNING_TIMELINE_ANALYSIS_INTERVAL_SECS',
    'DEFAULT_LEARNING_KB_REFLECTION_UTILITY_THRESHOLD',
    'learning-engine-policy',
    ':realtime-extraction-timeout-secs',
    ':cooccurrence-refresh-interval-secs',
  ]);

  requireAll(diagnostics, files.kbFacade, sources.kbFacade, [
    'mod analyze;',
    'mod args;',
    'mod beacon;',
    'mod code_search;',
    'mod compact;',
    'mod conflicts;',
    'mod discovery;',
    'mod gc;',
    'mod import;',
    'mod mutate;',
    'mod ops;',
    'mod quality;',
    'mod query;',
    'mod remember;',
    'use analyze::handle_kb_analyze;',
    'route_beacon_action',
    'handle_code_search',
    'use compact::handle_kb_compact;',
    'use discovery::handle_kb_discover;',
    'use gc::handle_kb_gc;',
    'use import::handle_kb_import;',
    'handle_kb_batch_forget',
    'handle_kb_batch_set_project',
    'handle_kb_forget',
    'handle_kb_update',
    'handle_kb_execute_plan',
    'handle_kb_queue_status',
    'handle_kb_analyze',
    'handle_kb_remember',
    'use query::{handle_kb_get, handle_kb_list, handle_kb_search};',
    'pub(crate) async fn handle',
    '"mission_kb_query"',
    '"mission_kb_mutate"',
    '"mission_kb_ops"',
    '"mission_beacon"',
    '"mission_kb_remember"',
  ]);

  requireAll(diagnostics, files.memory, sources.memory, [
    'MemoryKbRuntimeConfig',
    'load_memory_kb_config',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'pending_message_limit',
    'tool_result_preview_chars',
    'assistant_preview_chars',
    'get_pending_realtime_messages_with_limit(pending_msg_limit)',
  ]);

  requireAll(diagnostics, files.learningMod, sources.learningMod, [
    'LearningEngineRuntimeConfig',
    'decision_harvest_interval_secs',
    'cooccurrence_refresh_interval_secs',
    'V3 learning-engine-policy unavailable',
  ]);

  requireAll(diagnostics, files.learningExtraction, sources.learningExtraction, [
    'LearningEngineRuntimeConfig',
    'load_learning_engine_config',
    'realtime_extraction_timeout_ms',
    'kb_consolidation_interval_secs',
    'kb_auto_gc_interval_secs',
    'kb_reflection_interval_secs',
    'kb_reflection_utility_threshold',
    'kb_reflection_max_tokens',
  ]);

  requireAll(diagnostics, files.learningDecision, sources.learningDecision, [
    'LearningEngineRuntimeConfig',
    'decision_tier3_timeout_ms',
  ]);

  requireAll(diagnostics, files.learningTimeline, sources.learningTimeline, [
    'LearningEngineRuntimeConfig',
    'timeline_analysis_interval_secs',
    'timeline_window_arg',
    'timeline_error_limit',
    'timeline_llm_sample_limit',
    'timeline_slow_threshold_ms',
  ]);

  requireAll(diagnostics, files.learningIdle, sources.learningIdle, [
    'LearningEngineRuntimeConfig',
    'idle_explore_interval_secs',
  ]);

  requireAll(diagnostics, files.learningHistorical, sources.learningHistorical, [
    'LearningEngineRuntimeConfig',
    'habit_scan_interval_secs',
    'habit_scan_batch_size',
    'habit_scan_timeout_ms',
  ]);

  requireAll(diagnostics, files.kbArgs, sources.kbArgs, [
    'pub(super) struct KBRememberArgs',
    'pub(super) struct KBKeyArgs',
    'pub(super) struct KBUpdateArgs',
    'pub(super) struct KBSearchArgs',
    'pub(super) struct KBListArgs',
    'pub(super) struct KBImportArgs',
    'pub(super) struct KBDiscoverArgs',
    'pub(super) struct KBGCArgs',
    'lenient::option_i64',
    'fn default_list_limit()',
  ]);

  requireAll(diagnostics, files.kbQuality, sources.kbQuality, [
    'pub(super) fn check_content_quality',
    'architecture:summary',
    'summary 过长',
    'summary 为空',
    'test write',
    'batch-',
    'stack trace',
    'RUST_BACKTRACE',
    'detail 过长',
  ]);

  requireAll(diagnostics, files.kbRemember, sources.kbRemember, [
    'pub(super) async fn handle_kb_remember',
    'KBRememberArgs',
    'check_content_quality',
    'KBRememberInput',
    'EmbeddingTask::ProcessKBEntry',
    'consolidated_from',
    'kb_add_edge',
    'kb_add_ast_link',
    'KBBatchMutated',
    'detect_kb_conflicts',
    'kb_adjust_confidence',
    'contradicts',
  ]);

  requireAll(diagnostics, files.kbCompact, sources.kbCompact, [
    'pub(super) async fn handle_kb_compact',
    'dryRun',
    'kb_list(None)',
    'low_confidence',
    'stale_state',
    'stale_ops',
    'stale_debug',
    'stale_bugfix',
    'low_value_fact',
    'expired_scratchpad',
    'kb_batch_forget',
  ]);

  requireAll(diagnostics, files.kbConflicts, sources.kbConflicts, [
    'pub(super) async fn detect_kb_conflicts',
    'CONFLICT_SIM_THRESHOLD',
    'embedding_service',
    'cosine_similarity',
    'text_jaccard',
    'category_prefix',
    'conflicts.truncate(5)',
  ]);

  requireAll(diagnostics, files.kbQuery, sources.kbQuery, [
    'pub(super) async fn handle_kb_search',
    'pub(super) async fn handle_kb_get',
    'pub(super) async fn handle_kb_list',
    'KBSearchArgs',
    'KBKeyArgs',
    'KBListArgs',
    'kb_search_fts_ranked_scoped',
    'kb_search_like_ranked_scoped',
    'kb_search_cache',
    'rrf_score',
    'temporal_decay',
    'mmr_rerank_cosine',
    'kb_update_access_stats',
    'kb_get(&key)',
    'kb_list_paginated',
    '"compact": true',
    'Key not found',
  ]);

  requireAll(diagnostics, files.kbDiscovery, sources.kbDiscovery, [
    'pub(super) async fn handle_kb_discover',
    'KBDiscoverArgs',
    'state.infra.read()',
    'kb_search(&format!("{} password", host), Some("credential"))',
    'tokio::process::Command',
    'AsyncWriteExt',
    'StrictHostKeyChecking=no',
    'ConnectTimeout=10',
    'KBRememberInput',
    'source: Some("discovery".to_string())',
    'SSH probe failed',
  ]);

  requireAll(diagnostics, files.kbAnalyze, sources.kbAnalyze, [
    'pub(super) async fn handle_kb_analyze',
    'kb_list_paginated',
    'redact_sensitive',
    'include_board_context',
    'BoardTaskStatus::Done',
    'response_format',
    'kb_consolidation_actions',
    'apply_context_budget',
    'MAX_ROUTER_PAYLOAD_BYTES',
    'resolve_llm_credentials',
    'REQUEST_CALLER',
    'send_with_timeout',
    'kb_ops_save_plan',
    'KBOperation',
    'context_budget',
  ]);

  requireAll(diagnostics, files.kbMutate, sources.kbMutate, [
    'pub(super) async fn handle_kb_forget',
    'pub(super) async fn handle_kb_batch_forget',
    'pub(super) async fn handle_kb_batch_set_project',
    'pub(super) async fn handle_kb_update',
    'check_content_quality',
    'kb_get_id_by_key',
    'kb_batch_forget',
    'kb_update',
    'EmbeddingTask::ProcessKBEntry',
    'KBBatchMutated',
  ]);

  requireAll(diagnostics, files.kbImport, sources.kbImport, [
    'pub(super) async fn handle_kb_import',
    'KBImportArgs',
    'servers_yaml',
    'default_mission_home',
    'InfraConfig::load',
    'KBRememberInput',
    'Unsupported import format',
  ]);

  requireAll(diagnostics, files.kbGc, sources.kbGc, [
    'pub(super) async fn handle_kb_gc',
    'KBGCArgs',
    'kb_stats',
    'kb_find_stale',
    'kb_find_duplicates',
    'kb_batch_forget',
    'clean_stale',
    'clean_duplicates',
    'Unknown gc action',
  ]);

  requireAll(diagnostics, files.kbOps, sources.kbOps, [
    'pub(super) async fn handle_kb_queue_status',
    'pub(super) async fn handle_kb_execute_plan',
    'kb_ops_list',
    'kb_ops_plan_summary',
    'kb_ops_expire_stale',
    'kb_ops_update_status',
    'execute_delete',
    'execute_update',
    'execute_dispatch',
    'KBRememberInput',
    'publish_task',
    'TaskEvent::Created',
    'submit_task',
  ]);

  requireAll(diagnostics, files.kbBeacon, sources.kbBeacon, [
    'pub(super) fn route_beacon_action',
    'mission_beacon_map',
    'mission_beacon_tag',
    'mission_beacon_annotate',
    'feature',
    'pub(super) async fn handle_beacon_list',
    'pub(super) async fn handle_beacon_map',
    'pub(super) async fn handle_beacon_tag',
    'pub(super) async fn handle_beacon_annotate',
    'beacon_list',
    'beacon_map',
    'beacon_ensure',
    'beacon_node_upsert',
    'beacon_node_annotate',
    '@beacon:',
  ]);

  requireAll(diagnostics, files.kbCodeSearch, sources.kbCodeSearch, [
    'pub(super) async fn handle_code_search',
    'CodeSearchArgs',
    'ast_search',
    'node_type',
    'ast_find_related',
    'No code nodes found matching query',
    'No code nodes matched filters',
  ]);

  requireAll(diagnostics, files.mcpKb, sources.mcpKb, [
    '"mission_kb_query"',
    '"mission_kb_remember"',
    '"mission_kb_mutate"',
    '"mission_kb_ops"',
    '"mission_beacon"',
    '"mission_code_search"',
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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-memory-kb-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
	  (memory-kb-policy
	    :pending-message-limit 60
	    :tool-result-preview-chars 1000
	    :assistant-preview-chars 500)
	  (learning-engine-policy
	    :realtime-extraction-timeout-secs 300
	    :decision-tier3-timeout-secs 300
	    :habit-scan-timeout-secs 600
	    :timeline-analysis-interval-secs 43200
	    :cooccurrence-refresh-interval-secs 21600
	    :invariants ["LearningEngineRuntimeConfig MUST load learning-engine-policy"])
	  (implementation-map
    (surface memory-kb
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/args.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/remember.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/quality.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/compact.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/analyze.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/mutate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/import.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/gc.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/ops.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/beacon.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/kb/code_search.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/mod.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
	             "scripts/check-v3-memory-kb-isomorphism.mjs"]
	      :note "memory-kb-policy realtime extraction batch size and preview truncation budgets; projects learning-engine-policy into learning_engine pty send budgets, maintenance cadences, timeline read windows, and KB reflection policy; kb.rs remains the memory-kb facade; kb/args.rs owns unified KB argument ingress; kb/remember.rs owns remember ingestion, graph edge side effects, embedding trigger, mutation event, and conflict downweighting; kb/quality.rs owns content-quality rejection; kb/compact.rs owns rule-based KB compaction; kb/conflicts.rs owns semantic conflict detection; kb/query.rs owns search/get/list retrieval egress; kb/discovery.rs owns SSH probe discovery and infra KB projection; kb/analyze.rs owns LLM analysis, context-budgeting, and consolidation-plan queue projection; kb/mutate.rs owns forget/update/project mutation side effects; kb/import.rs owns servers_yaml import projection; kb/gc.rs owns stats/stale/duplicates cleanup actions; kb/ops.rs owns queue-status and execute-plan operation egress; kb/beacon.rs owns unified mission_beacon action routing plus legacy beacon list/map/tag/annotate; kb/code_search.rs owns AST code-search egress."))
  (compression-contract
    :checks ["node scripts/check-v3-memory-kb-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.kbFacade, `
mod analyze;
mod args;
mod beacon;
mod code_search;
mod compact;
mod conflicts;
mod discovery;
mod gc;
mod import;
mod mutate;
mod ops;
mod quality;
mod query;
mod remember;
use analyze::handle_kb_analyze;
route_beacon_action; handle_code_search;
use compact::handle_kb_compact;
use discovery::handle_kb_discover;
use gc::handle_kb_gc;
use import::handle_kb_import;
handle_kb_batch_forget; handle_kb_batch_set_project; handle_kb_forget; handle_kb_update;
handle_kb_execute_plan; handle_kb_queue_status;
handle_kb_analyze;
handle_kb_remember;
use query::{handle_kb_get, handle_kb_list, handle_kb_search};
pub(crate) async fn handle() {
  "mission_kb_query"; "mission_kb_mutate"; "mission_kb_ops"; "mission_beacon"; "mission_kb_remember";
}`);
	  writeFixture(root, DEFAULT_FILES.v3Runtime, `
	MemoryKbRuntimeConfig; LearningEngineRuntimeConfig; parse_memory_kb_policy; parse_learning_engine_policy; DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT; DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS; DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS; memory-kb-policy; :pending-message-limit; :tool-result-preview-chars; :assistant-preview-chars; DEFAULT_LEARNING_REALTIME_EXTRACTION_TIMEOUT_SECS; DEFAULT_LEARNING_TIMELINE_ANALYSIS_INTERVAL_SECS; DEFAULT_LEARNING_KB_REFLECTION_UTILITY_THRESHOLD; learning-engine-policy; :realtime-extraction-timeout-secs; :cooccurrence-refresh-interval-secs;
	`);
  writeFixture(root, DEFAULT_FILES.kbArgs, `
pub(super) struct KBRememberArgs;
pub(super) struct KBKeyArgs;
pub(super) struct KBUpdateArgs;
pub(super) struct KBSearchArgs;
pub(super) struct KBListArgs;
pub(super) struct KBImportArgs;
pub(super) struct KBDiscoverArgs;
pub(super) struct KBGCArgs;
lenient::option_i64;
fn default_list_limit() {}
`);
  writeFixture(root, DEFAULT_FILES.kbQuality, `
pub(super) fn check_content_quality() {
  architecture:summary; summary 过长; summary 为空; test write; batch-; stack trace; RUST_BACKTRACE; detail 过长;
}`);
  writeFixture(root, DEFAULT_FILES.kbRemember, `
pub(super) async fn handle_kb_remember() {
  KBRememberArgs; check_content_quality(); KBRememberInput; EmbeddingTask::ProcessKBEntry; consolidated_from; kb_add_edge(); kb_add_ast_link(); KBBatchMutated; detect_kb_conflicts(); kb_adjust_confidence(); contradicts;
}`);
  writeFixture(root, DEFAULT_FILES.kbCompact, `
pub(super) async fn handle_kb_compact() {
  dryRun; kb_list(None); low_confidence; stale_state; stale_ops; stale_debug; stale_bugfix; low_value_fact; expired_scratchpad; kb_batch_forget;
}`);
  writeFixture(root, DEFAULT_FILES.kbConflicts, `
pub(super) async fn detect_kb_conflicts() {
  CONFLICT_SIM_THRESHOLD; embedding_service; cosine_similarity; text_jaccard; category_prefix; conflicts.truncate(5);
}`);
  writeFixture(root, DEFAULT_FILES.kbQuery, `
pub(super) async fn handle_kb_search() {
  KBSearchArgs; kb_search_fts_ranked_scoped(); kb_search_like_ranked_scoped(); kb_search_cache; rrf_score(); temporal_decay(); mmr_rerank_cosine(); kb_update_access_stats();
}
pub(super) async fn handle_kb_get() {
  KBKeyArgs; kb_get(&key); Key not found;
}
pub(super) async fn handle_kb_list() {
  KBListArgs; kb_list_paginated(); "compact": true;
}`);
  writeFixture(root, DEFAULT_FILES.kbDiscovery, `
pub(super) async fn handle_kb_discover() {
  KBDiscoverArgs; state.infra.read(); kb_search(&format!("{} password", host), Some("credential")); tokio::process::Command; AsyncWriteExt; StrictHostKeyChecking=no; ConnectTimeout=10; KBRememberInput; source: Some("discovery".to_string()); SSH probe failed;
}`);
  writeFixture(root, DEFAULT_FILES.kbAnalyze, `
pub(super) async fn handle_kb_analyze() {
  kb_list_paginated(); redact_sensitive(); include_board_context; BoardTaskStatus::Done; response_format; kb_consolidation_actions; apply_context_budget(); MAX_ROUTER_PAYLOAD_BYTES; resolve_llm_credentials(); REQUEST_CALLER; send_with_timeout(); kb_ops_save_plan(); KBOperation; context_budget;
}`);
  writeFixture(root, DEFAULT_FILES.kbMutate, `
pub(super) async fn handle_kb_forget() { kb_get_id_by_key(); KBBatchMutated; }
pub(super) async fn handle_kb_batch_forget() { kb_batch_forget(); KBBatchMutated; }
pub(super) async fn handle_kb_batch_set_project() { kb_update(); }
pub(super) async fn handle_kb_update() {
  check_content_quality(); kb_update(); EmbeddingTask::ProcessKBEntry; KBBatchMutated;
}`);
  writeFixture(root, DEFAULT_FILES.kbImport, `
pub(super) async fn handle_kb_import() {
  KBImportArgs; servers_yaml; default_mission_home(); InfraConfig::load(); KBRememberInput; Unsupported import format;
}`);
  writeFixture(root, DEFAULT_FILES.kbGc, `
pub(super) async fn handle_kb_gc() {
  KBGCArgs; kb_stats(); kb_find_stale(); kb_find_duplicates(); kb_batch_forget(); clean_stale; clean_duplicates; Unknown gc action;
}`);
  writeFixture(root, DEFAULT_FILES.kbOps, `
pub(super) async fn handle_kb_queue_status() {
  kb_ops_list(); kb_ops_plan_summary();
}
pub(super) async fn handle_kb_execute_plan() {
  kb_ops_list(); kb_ops_expire_stale(); kb_ops_update_status(); execute_delete(); execute_update(); execute_dispatch(); KBRememberInput; publish_task(); TaskEvent::Created; submit_task();
}`);
  writeFixture(root, DEFAULT_FILES.kbBeacon, `
pub(super) fn route_beacon_action() { mission_beacon_map; mission_beacon_tag; mission_beacon_annotate; feature; }
pub(super) async fn handle_beacon_list() { beacon_list(); }
pub(super) async fn handle_beacon_map() { beacon_map(); }
pub(super) async fn handle_beacon_tag() { beacon_ensure(); beacon_node_upsert(); @beacon:; }
pub(super) async fn handle_beacon_annotate() { beacon_node_annotate(); }
`);
  writeFixture(root, DEFAULT_FILES.kbCodeSearch, `
pub(super) async fn handle_code_search() {
  CodeSearchArgs; ast_search(); node_type; ast_find_related(); No code nodes found matching query; No code nodes matched filters;
}`);
	  writeFixture(root, DEFAULT_FILES.memory, `
	MemoryKbRuntimeConfig; load_memory_kb_config; V3_BLUEPRINT_CONFIG_ERROR; pending_message_limit; tool_result_preview_chars; assistant_preview_chars; get_pending_realtime_messages_with_limit(pending_msg_limit);
	`);
	  writeFixture(root, DEFAULT_FILES.learningMod, `
	LearningEngineRuntimeConfig; decision_harvest_interval_secs; cooccurrence_refresh_interval_secs; V3 learning-engine-policy unavailable;
	`);
	  writeFixture(root, DEFAULT_FILES.learningExtraction, `
	LearningEngineRuntimeConfig; load_learning_engine_config; realtime_extraction_timeout_ms; kb_consolidation_interval_secs; kb_auto_gc_interval_secs; kb_reflection_interval_secs; kb_reflection_utility_threshold; kb_reflection_max_tokens;
	`);
	  writeFixture(root, DEFAULT_FILES.learningDecision, `
	LearningEngineRuntimeConfig; decision_tier3_timeout_ms;
	`);
	  writeFixture(root, DEFAULT_FILES.learningTimeline, `
	LearningEngineRuntimeConfig; timeline_analysis_interval_secs; timeline_window_arg; timeline_error_limit; timeline_llm_sample_limit; timeline_slow_threshold_ms;
	`);
	  writeFixture(root, DEFAULT_FILES.learningIdle, `
	LearningEngineRuntimeConfig; idle_explore_interval_secs;
	`);
	  writeFixture(root, DEFAULT_FILES.learningHistorical, `
	LearningEngineRuntimeConfig; habit_scan_interval_secs; habit_scan_batch_size; habit_scan_timeout_ms;
	`);
	  writeFixture(root, DEFAULT_FILES.mcpKb, `
"mission_kb_query"; "mission_kb_remember"; "mission_kb_mutate"; "mission_kb_ops"; "mission_beacon"; "mission_code_search";
`);
  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content.trimStart());
}

main();
