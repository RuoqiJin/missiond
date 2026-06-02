#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

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
  kbReview: 'crates/missiond-daemon/src/handlers/knowledge/kb/review.rs',
  contextGather: 'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
  mcpContextGather: 'crates/missiond-mcp/src/tools/knowledge/context_gather.rs',
  evidenceMigration: 'crates/missiond-core/migrations/20260602000000_evidence_lane_read_models.sql',
  runtimeDomains: 'scripts/lib/v3_runtime_domains.mjs',
  runtimeConfigPayload: 'crates/missiond-daemon/src/context/v3_blueprint_runtime/runtime_config_payload.rs',
  lispcEmit: 'tools/missiond_lispc/bin/emit_json.ml',
  toolDirectory: 'crates/missiond-daemon/src/handlers/comm/tool_directory.rs',
  kbReviewMigration: 'crates/missiond-core/migrations/20260508001000_knowledge_review_state.sql',
  kbReviewTypes: 'crates/missiond-core/src/types/knowledge.rs',
  kbReviewTraits: 'crates/missiond-core/src/db/traits.rs',
  kbReviewPg: 'crates/missiond-core/src/db/pg/knowledge.rs',
  memory: 'crates/missiond-daemon/src/handlers/knowledge/memory.rs',
  learningMod: 'crates/missiond-daemon/src/engine/learning_engine/mod.rs',
  learningExtraction: 'crates/missiond-daemon/src/engine/learning_engine/extraction.rs',
  learningDecision: 'crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs',
  learningTimeline: 'crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs',
  learningIdle: 'crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs',
  learningHistorical: 'crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs',
  experienceHarvester: 'crates/missiond-daemon/src/workers/local/experience_harvester.rs',
  strategyWorker: 'crates/missiond-daemon/src/workers/gemini/strategy_worker.rs',
  pgConversation: 'crates/missiond-core/src/db/pg/conversation.rs',
  eventProjection: 'crates/missiond-core/src/event/projection.rs',
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
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
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
	    ':sensitive-query-suppression [architecture:module]',
	    ':realtime-extraction-timeout-secs 300',
	    ':realtime-empty-backoff-base-secs 30',
	    ':realtime-empty-backoff-max-secs 900',
	    ':deep-analysis-zero-output-fuse-threshold 3',
	    ':deep-analysis-zero-output-fuse-secs 3600',
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
	    'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
	    'crates/missiond-mcp/src/tools/knowledge/context_gather.rs',
	    'crates/missiond-daemon/src/handlers/comm/tool_directory.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/mod.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/extraction.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs',
	    'crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs',
	    'crates/missiond-core/src/db/pg/conversation.rs',
	    'scripts/check-v3-memory-kb-isomorphism.mjs',
	    'memory-kb-policy realtime extraction batch size and preview truncation budgets',
	    'mission_memory_pending MUST cache the served realtime extraction batch',
	    'mission_memory_pending MUST classify deployment-monitor',
	    'deployment-monitor covers deploy/build/smoke/rollback/agent-update/provenance diagnostics',
	    'mission_kb_query MUST suppress architecture:module details for sensitive credential/secret/SSH/token queries',
	    'mission_kb_query MUST support excludeCategory / exclude_category',
	    'mission_kb_mutate(action=batch_remember) MUST accept a bounded entries array',
	    'mission_kb_remember MUST pass through one shared dedupe gate',
	    'Realtime extraction MUST apply exponential empty-queue backoff from learning-engine-policy',
	    'Deep analysis MUST apply a Lisp-projected zero-output saturation fuse',
	    'token_usage_ledger through a Lisp-projected sliding-window token-spend soft guard',
	    ':token-spend-guard-window-secs',
	    ':token-spend-guard-soft-limit',
	    'knowledge_review_state overlay',
    'projects learning-engine-policy into learning_engine pty send budgets, maintenance cadences, timeline read windows, and KB reflection policy',
    'Realtime extraction MUST claim the extraction lane before running pending-message DB probes',
    'pending realtime SQL MUST use EXISTS/LATERAL LIMIT or bounded materialized-candidate shapes instead of global COUNT(DISTINCT)/ROW_NUMBER scans',
    'deep-analysis active-conversation probes MUST use bounded EXISTS/OFFSET checks instead of full message COUNT scans',
    'Memory extraction pending selectors MUST filter MissionD self-referential worker slots',
    'Timeline projection SQL MUST cast string-bound since/until parameters as ::timestamptz when comparing against event_log.ts',
    'Timeline Analyst MUST check the Gemini provider gate before collecting timeline evidence or calling Gemini',
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
    'kb/review.rs owns non-destructive knowledge_review_state overlay',
    ':review-states [active superseded-by-lisp superseded-by-code historical-evidence duplicate wrong-or-stale delete-candidate needs-human]',
    'mission_kb_review MUST write a non-destructive knowledge_review_state overlay; it MUST NOT mutate or delete the original knowledge row.',
    'mission_kb_query default retrieval MUST honor the review overlay while include_archived=true and state_filter preserve audit access to historical evidence.',
    'grounding-search-aggregate',
    'evidence-lane-policy',
    'runtime_truth',
    'project_ssot',
    'reviewed_kb',
    'active_board',
    'skill_evidence',
    'conversation_audit',
    'cold_archive',
    'support_refs',
    'evidence_items',
    'context_gather_runs',
    'conversation_episodes',
    'conversation_fact_extracts',
    'skill_evidence_items',
    'source_profile',
    'intent_default',
    'deploy_ops',
    'conversation_audit',
    'full_debug',
    'evidence_lanes',
    'authority_order',
    'noise_diagnostics',
    'context_noise_metrics',
    'include_raw_sources',
    'persist_read_model',
    'credential_refs MUST NOT be emitted unless include_credentials=true',
    'skill-edit-delegation-policy',
    'task-record-indexing',
    'active Board task records',
    'bounded conversation logs',
    'Worker context packs MUST include evidence_lanes, evidence_items, and support_catalog lane summaries by default and omit raw sources',
    'mission_context_gather MUST normalize legacy source calls into typed EvidenceItem lanes',
    'mission_context_gather(persist_read_model=true, the default) MUST persist context_gather_runs metrics and evidence_items compact projections',
    'persist=true additionally creates the context-pack artifact/capsule and forces read-model persistence',
    'mission_context_gather source_profile=deploy_ops infra skill_evidence MUST recognize deployment-closure evidence anchors',
    'skill-file context fallback may admit sibling evidence only when the returned line itself carries a strong closure anchor',
    'source_summaries.infra.status=feature_disabled',
    'optional feature_disabled diagnostics MUST NOT set the top-level ok=false by themselves',
    'mission_context_gather support_catalog MUST project compiled service runtime plus compiled-deployment-policy into deployment_closure evidence',
    'deployment_closure_policy',
    'ReleaseLease',
    'RuntimeObservation',
    'ReleaseEvidence',
    'ClosureVerdict',
    'mission_context_gather MUST aggregate runtime_environment, KB, active SSOT, project registry, skill operational evidence, infra evidence, active Board task records, and bounded conversation logs through authority-aware evidence lanes',
    'Board/task/workflow records are searchable retrieval evidence',
    'Mutating skill files under ~/.claude/skills, ~/.codex/skills, or project skill directories MUST be represented as a BoardTask/work-order and delegated to a ClaudeCode skill-maintainer or deploy-ops lane.',
    'mission_kb_query MUST support excludeCategory / exclude_category for explicit category suppression',
    'mission_kb_mutate(action=batch_remember) MUST accept a bounded entries array',
    'mission_kb_remember MUST pass through one shared dedupe gate',
    'Low-confidence semantic duplicate candidates MUST create a needs-human knowledge_review_state artifact',
    'apply knowledge_review_state overlay before default retrieval so superseded/historical/duplicate/stale/delete-candidate memories leave the active reasoning path without deletion',
    'crates/missiond-core/migrations/20260508001000_knowledge_review_state.sql',
    'crates/missiond-core/src/types/knowledge.rs',
    'crates/missiond-core/src/db/traits.rs',
    'crates/missiond-core/src/db/pg/knowledge.rs',
    'node scripts/check-v3-memory-kb-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.contextGather, sources.contextGather, [
    'SourceProfile',
    'source_profile',
    'source_selection',
    'include_credentials',
    'include_raw_sources',
    'include_board',
    'include_conversations',
    'conversation_time_range',
    'evidence_lanes',
    'evidence_items',
    'support_catalog',
    'deployment_closure',
    'deployment_closure_policy',
    'load_compiled_project_universe',
    'compiled_service_runtime_payload_for_project',
    'supportCatalog',
    'compiled_deployment_policy_for_service',
    'build_deployment_closure_support',
    'authority_order',
    'noise_diagnostics',
    'context_noise_metrics',
    'build_support_catalog',
    'build_evidence_items',
    'persist_evidence_lane_projection',
    'context_gather_persist_read_model',
    'persistReadModel',
    'record_context_gather_run',
    'upsert_evidence_items',
    'runtime_truth',
    'project_ssot',
    'reviewed_kb',
    'active_board',
    'skill_evidence',
    'conversation_audit',
    'cold_archive',
    'support_refs',
    'context_pack_artifact_payload',
    'credential_lane_opt_in',
    'selection.include_credentials',
    'selection.include_raw_sources',
    'raw_sources_omitted',
    '"board_tasks"',
    '"conversation_logs"',
    '"credential_refs"',
    '"mission_board_query"',
    '"mission_conversation_query"',
    '"scope": "active"',
    '"time_range"',
    'last_30d',
  ]);

  requireAll(diagnostics, files.mcpContextGather, sources.mcpContextGather, [
    'runtime_truth',
    'project_ssot',
    'reviewed_kb',
    'active_board',
    'support_refs',
    'skill_evidence',
    'conversation_audit',
    'cold_archive',
    'evidence_lanes',
    'evidence_items',
    'support_catalog',
    'source_profile',
    'sourceProfile',
    'intent_default',
    'deploy_ops',
    'conversation_audit',
    'full_debug',
    'include_credentials',
    'includeCredentials',
    'include_raw_sources',
    'includeRawSources',
    'persist',
    'persist_read_model',
    'persistReadModel',
    'include_board',
    'include_conversations',
    'conversation_time_range',
  ]);

  requireAll(diagnostics, files.toolDirectory, sources.toolDirectory, [
    'mission_context_gather + mission_conversation_* + mission_timeline + mission_audit',
    '"mission_context_gather"',
    '"grounding"',
    '"intent"',
  ]);

  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'MemoryKbRuntimeConfig',
    'EvidenceLaneRuntimeConfig',
    'EvidenceLaneRuntimeEntry',
    'EvidenceLaneProfileRuntimeEntry',
    'evidence-lane-policy',
    'payload.evidence_lane_policy.clone()',
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
    'DEFAULT_LEARNING_REALTIME_EMPTY_BACKOFF_BASE_SECS',
    'DEFAULT_LEARNING_REALTIME_EMPTY_BACKOFF_MAX_SECS',
    'DEFAULT_LEARNING_DEEP_ANALYSIS_ZERO_OUTPUT_FUSE_THRESHOLD',
    'DEFAULT_LEARNING_DEEP_ANALYSIS_ZERO_OUTPUT_FUSE_SECS',
    'DEFAULT_LEARNING_TOKEN_SPEND_GUARD_WINDOW_SECS',
    'DEFAULT_LEARNING_TOKEN_SPEND_GUARD_SOFT_LIMIT',
    'DEFAULT_LEARNING_TIMELINE_ANALYSIS_INTERVAL_SECS',
    'DEFAULT_LEARNING_KB_REFLECTION_UTILITY_THRESHOLD',
    'learning-engine-policy',
    ':realtime-extraction-timeout-secs',
    ':realtime-empty-backoff-base-secs',
    ':realtime-empty-backoff-max-secs',
    ':deep-analysis-zero-output-fuse-threshold',
    ':deep-analysis-zero-output-fuse-secs',
    ':token-spend-guard-window-secs',
    ':token-spend-guard-soft-limit',
    ':cooccurrence-refresh-interval-secs',
  ]);

  requireAll(diagnostics, files.runtimeConfigPayload, sources.runtimeConfigPayload, [
    'evidenceLanePolicy',
    'evidence_lane_policy',
    'EvidenceLaneRuntimeConfig',
    '#[serde(rename = "evidenceLanePolicy", default)]',
  ]);

  requireAll(diagnostics, files.runtimeDomains, sources.runtimeDomains, [
    'evidence-lane-policy',
    'evidenceLanePolicy',
    'compiled-runtime-evidence-lane-policy.json',
  ]);

  requireAll(diagnostics, files.lispcEmit, sources.lispcEmit, [
    'evidence_lane_policy_runtime_config_json',
    'evidence_lane_entry_json',
    'evidence_lane_profile_json',
    'evidenceLanePolicy',
    'find_child root "evidence-lane-policy"',
  ]);

  requireAll(diagnostics, files.evidenceMigration, sources.evidenceMigration, [
    'CREATE TABLE IF NOT EXISTS evidence_items',
    'CREATE TABLE IF NOT EXISTS context_gather_runs',
    'CREATE TABLE IF NOT EXISTS conversation_episodes',
    'CREATE TABLE IF NOT EXISTS conversation_fact_extracts',
    'CREATE TABLE IF NOT EXISTS conversation_duplicate_groups',
    'CREATE TABLE IF NOT EXISTS skill_evidence_items',
    "'runtime_truth'",
    "'project_ssot'",
    "'reviewed_kb'",
    "'active_board'",
    "'skill_evidence'",
    "'conversation_audit'",
    "'cold_archive'",
    "'support_refs'",
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
    'mod review;',
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
    'handle_kb_review',
    'use query::{handle_kb_get, handle_kb_list, handle_kb_search};',
    'pub(crate) async fn handle',
    '"mission_kb_query"',
    '"mission_kb_mutate"',
    '"mission_kb_ops"',
    '"mission_kb_review"',
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
    'MAX_PENDING_BATCH_REPLAYS',
    'classify_memory_input_noise',
    'deployment-monitor',
    'deployment-event-response',
    'xjp_build_wait',
    'xjp_deploy_watch',
    'build_started',
    'agent_update_failed',
    'reported_digest_missing',
    'runtime-report',
    'worker-instruction',
    'provider-preamble',
    'inputSkipDiagnostics',
    'inputFilter',
    'mark_pending_batch_served',
    'pending_payload',
    'MEMORY_PENDING_ALREADY_SERVED',
    'ToolResult::structured_error',
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
    'try_claim_extraction_probe',
    'release_extraction_probe',
    'should_skip_realtime_empty_backoff',
    'should_skip_deep_analysis_zero_output_fuse',
    'should_skip_memory_due_to_token_spend_guard',
    'token_spend_guard_window_secs',
    'token_spend_guard_soft_limit',
    'token_stats(None, Some(slot_id)',
    'CtlDomain::Memory',
    'record_deep_analysis_completion',
    'deep_analysis_zero_output_fuse_threshold',
    'record_realtime_empty_probe',
    'reset_realtime_empty_backoff',
    'another extraction probe already claimed the lane',
    'kb_consolidation_interval_secs',
    'kb_auto_gc_interval_secs',
    'kb_reflection_interval_secs',
    'kb_reflection_utility_threshold',
    'kb_reflection_max_tokens',
  ]);

  requireAll(diagnostics, files.pgConversation, sources.pgConversation, [
    'EXISTS (',
    'CROSS JOIN LATERAL',
    'LIMIT 15',
    'WITH candidate AS MATERIALIZED',
    'LIMIT 2000',
    'OFFSET 99',
    "slot_id NOT LIKE 'slot-memory%'",
    "slot_id NOT LIKE 'slot-diagnosis%'",
    "slot_id NOT LIKE 'agent-%'",
  ]);
  forbidAll(diagnostics, files.pgConversation, sources.pgConversation, [
    'COUNT(DISTINCT c.id) FROM conversations c\n             JOIN conversation_messages',
    'ROW_NUMBER() OVER(PARTITION BY m.session_id',
    "SELECT COUNT(*) FROM conversation_messages m\n                    WHERE m.session_id = conversations.id",
    "SELECT COUNT(*) FROM conversation_messages m\n                       WHERE m.session_id = conversations.id",
  ]);

  requireAll(diagnostics, files.learningDecision, sources.learningDecision, [
    'LearningEngineRuntimeConfig',
    'decision_tier3_timeout_ms',
  ]);

  requireAll(diagnostics, files.learningTimeline, sources.learningTimeline, [
    'LearningEngineRuntimeConfig',
    'UpsertTaskContractCommand',
    'timeline_insight_runtime_metadata',
    '"control_state": "task_contracts"',
    '"sandbox_profile": "system-learning-review"',
    'llm_gate::is_disabled',
    'LlmProvider::Gemini',
    'Timeline Analyst: skipped because Gemini gate is closed',
    'last_timeline_analysis_at',
    'timeline_analysis_interval_secs',
    'timeline_window_arg',
    'timeline_error_limit',
    'timeline_llm_sample_limit',
    'timeline_slow_threshold_ms',
  ]);

  requireAll(diagnostics, files.eventProjection, sources.eventProjection, [
    'ts >= ${}::timestamptz',
    'ts <= ${}::timestamptz',
    'WHERE ts >= $1::timestamptz AND ts <= $2::timestamptz',
  ]);
  forbidAll(diagnostics, files.eventProjection, sources.eventProjection, [
    'format!("ts >= ${}", param_idx)',
    'format!("ts <= ${}", param_idx)',
    'format!("ts >= ${}", idx)',
    'format!("ts <= ${}", idx)',
    'WHERE ts >= $1 AND ts <= $2',
  ]);

  requireAll(diagnostics, files.learningIdle, sources.learningIdle, [
    'LearningEngineRuntimeConfig',
    'UpsertTaskContractCommand',
    'idle_exploration_runtime_metadata',
    '"control_state": "task_contracts"',
    '"sandbox_profile": "system-learning-review"',
    'auto_execute: Some(false)',
    'idle_explore_interval_secs',
  ]);

  requireAll(diagnostics, files.experienceHarvester, sources.experienceHarvester, [
    'UpsertTaskContractCommand',
    'skill_synthesis_runtime_metadata',
    '"control_state": "task_contracts"',
    '"sandbox_profile": "system-learning-review"',
    'auto_execute: Some(false)',
    'skill_synthesis_metadata_declares_task_contract_authority',
  ]);

  requireAll(diagnostics, files.strategyWorker, sources.strategyWorker, [
    'UpsertTaskContractCommand',
    'strategy_skill_review_runtime_metadata',
    'strategy_drift_review_runtime_metadata',
    '"control_state": "task_contracts"',
    '"sandbox_profile": "system-learning-review"',
    'auto_execute: Some(false)',
    'strategy_skill_metadata_declares_task_contract_authority',
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
    'pub(super) struct KBReviewArgs',
    'pub(super) include_archived: bool',
    'pub(super) state_filter: Option<String>',
    'pub(super) exclude_category: Option<Value>',
    'pub(super) struct KBBatchRememberArgs',
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
    'write_duplicate_review_artifact',
    'KnowledgeReviewInput',
    'needs-human',
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
    'fn review_state_hidden',
    'async fn filter_entries_by_review',
    'kb_review_current_for_ids',
    'kb_review_get_by_key',
    'include_archived',
    'state_filter',
    'exclude_category',
    '"unreviewed"',
    'parse_excluded_categories',
    'category_is_excluded',
    'is_sensitive_retrieval_intent',
    'suppress_for_sensitive_retrieval',
    'architecture:module',
    'Key is archived by KB review overlay',
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

  requireAll(diagnostics, files.kbReview, sources.kbReview, [
    'pub(super) async fn handle_kb_review',
    'KnowledgeReviewInput',
    'VALID_REVIEW_STATES',
    '"active"',
    '"superseded-by-lisp"',
    '"superseded-by-code"',
    '"historical-evidence"',
    '"duplicate"',
    '"wrong-or-stale"',
    '"delete-candidate"',
    '"needs-human"',
    '"upsert"',
    '"get"',
    '"stats"',
    'kb_review_upsert',
    'kb_review_get_by_key',
    'kb_review_current_for_ids',
    'kb_review_stats',
    'resolve_knowledge_id',
    'non_destructive',
  ]);

  requireAll(diagnostics, files.kbReviewMigration, sources.kbReviewMigration, [
    'CREATE TABLE IF NOT EXISTS knowledge_review_state',
    'knowledge_id TEXT NOT NULL REFERENCES knowledge(id) ON DELETE CASCADE',
    "'active'",
    "'superseded-by-lisp'",
    "'superseded-by-code'",
    "'historical-evidence'",
    "'duplicate'",
    "'wrong-or-stale'",
    "'delete-candidate'",
    "'needs-human'",
    'batch_id TEXT NOT NULL',
    'reviewer TEXT NOT NULL',
    'rationale TEXT NOT NULL',
    'evidence_refs JSONB NOT NULL',
    'is_current BOOLEAN NOT NULL DEFAULT TRUE',
    'CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_review_state_current',
    'WHERE is_current',
  ]);

  requireAll(diagnostics, files.kbReviewTypes, sources.kbReviewTypes, [
    'pub struct KnowledgeReviewState',
    'pub struct KnowledgeReviewInput',
    'pub knowledge_id: String',
    'pub state: String',
    'pub batch_id: String',
    'pub reviewer: String',
    'pub rationale: String',
    'pub evidence_refs: serde_json::Value',
    'pub superseded_by: Option<String>',
    'pub confidence: f64',
    'pub is_current: bool',
  ]);

  requireAll(diagnostics, files.kbReviewTraits, sources.kbReviewTraits, [
    'async fn kb_review_upsert',
    'async fn kb_review_current_for_ids',
    'async fn kb_review_get_by_key',
    'async fn kb_review_stats',
    'KnowledgeReviewInput',
    'KnowledgeReviewState',
  ]);

  requireAll(diagnostics, files.kbReviewPg, sources.kbReviewPg, [
    'async fn kb_review_upsert',
    'async fn kb_review_current_for_ids',
    'async fn kb_review_get_by_key',
    'async fn kb_review_stats',
    'INSERT INTO knowledge_review_state',
    'SET is_current = FALSE',
    'WHERE is_current = TRUE AND knowledge_id = ANY($1)',
    'JOIN knowledge k ON k.id = r.knowledge_id',
    'GROUP BY state',
    'SAME_SESSION_FUZZY_MERGE_THRESHOLD',
    'merge_detail_for_dedupe',
    'same_source_session',
    '_dedupe_merge_events',
  ]);

  requireAll(diagnostics, files.mcpKb, sources.mcpKb, [
    '"mission_kb_query"',
    '"mission_kb_remember"',
    '"mission_kb_mutate"',
    '"mission_kb_review"',
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

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden contract text present: ${needle}` });
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
	    :assistant-preview-chars 500
	    :sensitive-query-suppression [architecture:module]
	    :review-states [active superseded-by-lisp superseded-by-code historical-evidence duplicate wrong-or-stale delete-candidate needs-human]
	    :invariants ["mission_memory_pending MUST cache the served realtime extraction batch"
	                 "mission_kb_query MUST suppress architecture:module details for sensitive credential/secret/SSH/token queries"
	                 "mission_kb_review MUST write a non-destructive knowledge_review_state overlay; it MUST NOT mutate or delete the original knowledge row."
	                 "mission_kb_remember MUST pass through one shared dedupe gate in KbStore::kb_remember before any realtime/deep-analysis/manual pipeline can create a new active key."
	                 "Low-confidence semantic duplicate candidates MUST create a needs-human knowledge_review_state artifact."
	                 "mission_kb_query default retrieval MUST honor the review overlay while include_archived=true and state_filter preserve audit access to historical evidence."])
	  (function knowledge-memory
	    :surface memory-kb
	    :core ((step s4 :logic "route all realtime extraction, deep-analysis, manual MCP, and internal learning writes through the shared KB dedupe gate before any new active key can be created")
	           (step s5 :logic "apply knowledge_review_state overlay before default retrieval so superseded/historical/duplicate/stale/delete-candidate memories leave the active reasoning path without deletion")))
	  (learning-engine-policy
	    :realtime-extraction-timeout-secs 300
	    :realtime-empty-backoff-base-secs 30
	    :realtime-empty-backoff-max-secs 900
	    :deep-analysis-zero-output-fuse-threshold 3
	    :deep-analysis-zero-output-fuse-secs 3600
	    :decision-tier3-timeout-secs 300
	    :habit-scan-timeout-secs 600
	    :timeline-analysis-interval-secs 43200
	    :cooccurrence-refresh-interval-secs 21600
	    :invariants ["LearningEngineRuntimeConfig MUST load learning-engine-policy"
	                 "Realtime extraction MUST apply exponential empty-queue backoff from learning-engine-policy"
	                 "Deep analysis MUST apply a Lisp-projected zero-output saturation fuse"
	                 "mission_memory_pending MUST classify deployment-monitor"
	                 "Timeline projection SQL MUST cast string-bound since/until parameters as ::timestamptz when comparing against event_log.ts"
	                 "Timeline Analyst MUST check the Gemini provider gate before collecting timeline evidence or calling Gemini"])
	  (grounding-search-aggregate
	    :source_profile [intent_default deploy_ops conversation_audit full_debug]
	    :fields [evidence_lanes evidence_items support_catalog authority_order noise_diagnostics context_noise_metrics include_raw_sources persist_read_model]
	    :invariants ["credential_refs MUST NOT be emitted unless include_credentials=true"
		                 "Worker context packs MUST include evidence_lanes, evidence_items, and support_catalog lane summaries by default and omit raw sources"
		                 "mission_context_gather MUST normalize legacy source calls into typed EvidenceItem lanes"
		                 "mission_context_gather(persist_read_model=true, the default) MUST persist context_gather_runs metrics and evidence_items compact projections"
		                 "persist=true additionally creates the context-pack artifact/capsule and forces read-model persistence"
		                 "mission_context_gather source_profile=deploy_ops infra skill_evidence MUST recognize deployment-closure evidence anchors"
		                 "skill-file context fallback may admit sibling evidence only when the returned line itself carries a strong closure anchor"
		                 "source_summaries.infra.status=feature_disabled"
		                 "optional feature_disabled diagnostics MUST NOT set the top-level ok=false by themselves"
		                 "mission_context_gather support_catalog MUST project compiled service runtime plus compiled-deployment-policy into deployment_closure evidence"
		                 "mission_context_gather MUST aggregate runtime_environment, KB, active SSOT, project registry, skill operational evidence, infra evidence, active Board task records, and bounded conversation logs through authority-aware evidence lanes"
	                 "Board/task/workflow records are searchable retrieval evidence"
	                 "Mutating skill files under ~/.claude/skills, ~/.codex/skills, or project skill directories MUST be represented as a BoardTask/work-order and delegated to a ClaudeCode skill-maintainer or deploy-ops lane."])
	  (evidence-lane-policy
	    :schema "missiond.evidence-lane-policy.v1"
	    :primary-read-model evidence_items
	    :run-metrics-read-model context_gather_runs
	    (lane runtime_truth :authority-class runtime_truth :source-types [runtime_environment] :default-profiles [intent_default] :raw-policy compact_only :privacy-class operational :validity [current_rule] :freshness hot_runtime :injectable-by-default true :promotion-rules [already-authoritative])
	    (lane project_ssot :authority-class file_first_lisp_and_compiled_project_universe :source-types [project_resolution project_registry ssot] :default-profiles [intent_default] :raw-policy compact_only :privacy-class internal :validity [current_rule] :freshness compiled_runtime_bound :injectable-by-default true :promotion-rules [already-authoritative])
	    (lane reviewed_kb :authority-class knowledge_review_state :source-types [knowledge] :default-profiles [intent_default] :raw-policy compact_only :privacy-class internal :validity [active_fact] :freshness ttl_or_version_bound :injectable-by-default true :promotion-rules [review-required])
	    (lane active_board :authority-class board_projection :source-types [board_task] :default-profiles [intent_default] :raw-policy compact_only :privacy-class internal :validity [current_state] :freshness active_task_bound :injectable-by-default true :promotion-rules [artifact-before-kb])
	    (lane skill_evidence :authority-class evidence_only :source-types [skill_metadata skill_procedure skill_operational_fact skill_warning skill_credential_ref] :default-profiles [deploy_ops] :raw-policy compact_only :privacy-class internal :validity [evidence_only] :freshness version_bound_or_historical :injectable-by-default false :promotion-rules [needs_review-before-kb])
	    (lane conversation_audit :authority-class provider_durable_conversation_read_model :source-types [conversation_episode conversation_fact_extract conversation_duplicate_group] :default-profiles [conversation_audit] :raw-policy raw_opt_in_only :privacy-class audit :validity [derived_from_conversation] :freshness time_range_bound :injectable-by-default false :promotion-rules [episode-first])
	    (lane cold_archive :authority-class forensics_only_cold_archive :source-types [archived_session true_user_utterance transcript_dump research_dump raw_provider_log] :default-profiles [full_debug] :raw-policy explicit_path_or_full_debug_only :privacy-class audit :validity [historical_evidence] :freshness cold_archive :injectable-by-default false :promotion-rules [never-default])
	    (lane support_refs :authority-class redacted_support_catalog :source-types [support_catalog deployment_closure_policy release_lease runtime_observation release_evidence closure_verdict secret_ref] :default-profiles [intent_default] :raw-policy secret_refs_only :privacy-class reference :validity [current_reference] :freshness runtime_or_catalog_bound :injectable-by-default true :promotion-rules [secret-values-never-indexed deploy-closure-verdict-required])
	    :read-models ((table conversation_episodes) (table conversation_fact_extracts) (table skill_evidence_items))
	    :invariants ["support_refs MUST expose secret_ref namespace/key/provenance/availability only"])
	  (skill-edit-delegation-policy)
	  (task-record-indexing :records ["active Board task records" "bounded conversation logs"])
	  (fixture-contract-text
	    "deployment-monitor covers deploy/build/smoke/rollback/agent-update/provenance diagnostics"
	    "ReleaseLease"
	    "RuntimeObservation"
	    "ReleaseEvidence"
	    "ClosureVerdict"
	    "mission_kb_query MUST support excludeCategory / exclude_category"
	    "mission_kb_query MUST support excludeCategory / exclude_category for explicit category suppression"
	    "mission_kb_mutate(action=batch_remember) MUST accept a bounded entries array"
	    "token_usage_ledger through a Lisp-projected sliding-window token-spend soft guard"
	    ":token-spend-guard-window-secs"
	    ":token-spend-guard-soft-limit"
	    "Memory extraction pending selectors MUST filter MissionD self-referential worker slots")
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
	             "crates/missiond-daemon/src/handlers/knowledge/kb/review.rs"
	             "crates/missiond-core/migrations/20260508001000_knowledge_review_state.sql"
	             "crates/missiond-core/src/types/knowledge.rs"
	             "crates/missiond-core/src/db/traits.rs"
	             "crates/missiond-core/src/db/pg/knowledge.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/mod.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
	             "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
	             "crates/missiond-core/src/db/pg/conversation.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
	             "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
	             "crates/missiond-daemon/src/handlers/comm/tool_directory.rs"
	             "scripts/check-v3-memory-kb-isomorphism.mjs"]
	      :note "memory-kb-policy realtime extraction batch size and preview truncation budgets; knowledge_review_state overlay; projects learning-engine-policy into learning_engine pty send budgets, maintenance cadences, timeline read windows, and KB reflection policy; Realtime extraction MUST claim the extraction lane before running pending-message DB probes; pending realtime SQL MUST use EXISTS/LATERAL LIMIT or bounded materialized-candidate shapes instead of global COUNT(DISTINCT)/ROW_NUMBER scans; deep-analysis active-conversation probes MUST use bounded EXISTS/OFFSET checks instead of full message COUNT scans; kb.rs remains the memory-kb facade; kb/args.rs owns unified KB argument ingress; kb/remember.rs owns remember ingestion, graph edge side effects, embedding trigger, mutation event, and conflict downweighting; kb/quality.rs owns content-quality rejection; kb/compact.rs owns rule-based KB compaction; kb/conflicts.rs owns semantic conflict detection; kb/query.rs owns search/get/list retrieval egress; kb/discovery.rs owns SSH probe discovery and infra KB projection; kb/analyze.rs owns LLM analysis, context-budgeting, and consolidation-plan queue projection; kb/mutate.rs owns forget/update/project mutation side effects; kb/import.rs owns servers_yaml import projection; kb/gc.rs owns stats/stale/duplicates cleanup actions; kb/ops.rs owns queue-status and execute-plan operation egress; kb/beacon.rs owns unified mission_beacon action routing plus legacy beacon list/map/tag/annotate; kb/code_search.rs owns AST code-search egress; kb/review.rs owns non-destructive knowledge_review_state overlay."))
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
mod review;
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
handle_kb_review;
use query::{handle_kb_get, handle_kb_list, handle_kb_search};
pub(crate) async fn handle() {
  "mission_kb_query"; "mission_kb_mutate"; "mission_kb_ops"; "mission_kb_review"; "mission_beacon"; "mission_kb_remember";
}`);
	  writeFixture(root, DEFAULT_FILES.v3Runtime, `
	MemoryKbRuntimeConfig; EvidenceLaneRuntimeConfig; EvidenceLaneRuntimeEntry; EvidenceLaneProfileRuntimeEntry; evidence-lane-policy; payload.evidence_lane_policy.clone(); LearningEngineRuntimeConfig; parse_memory_kb_policy; parse_learning_engine_policy; DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT; DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS; DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS; memory-kb-policy; :pending-message-limit; :tool-result-preview-chars; :assistant-preview-chars; DEFAULT_LEARNING_REALTIME_EXTRACTION_TIMEOUT_SECS; DEFAULT_LEARNING_REALTIME_EMPTY_BACKOFF_BASE_SECS; DEFAULT_LEARNING_REALTIME_EMPTY_BACKOFF_MAX_SECS; DEFAULT_LEARNING_DEEP_ANALYSIS_ZERO_OUTPUT_FUSE_THRESHOLD; DEFAULT_LEARNING_DEEP_ANALYSIS_ZERO_OUTPUT_FUSE_SECS; DEFAULT_LEARNING_TOKEN_SPEND_GUARD_WINDOW_SECS; DEFAULT_LEARNING_TOKEN_SPEND_GUARD_SOFT_LIMIT; DEFAULT_LEARNING_TIMELINE_ANALYSIS_INTERVAL_SECS; DEFAULT_LEARNING_KB_REFLECTION_UTILITY_THRESHOLD; learning-engine-policy; :realtime-extraction-timeout-secs; :realtime-empty-backoff-base-secs; :realtime-empty-backoff-max-secs; :deep-analysis-zero-output-fuse-threshold; :deep-analysis-zero-output-fuse-secs; :token-spend-guard-window-secs; :token-spend-guard-soft-limit; :cooccurrence-refresh-interval-secs;
	`);
  writeFixture(root, DEFAULT_FILES.runtimeConfigPayload, `
#[serde(rename = "evidenceLanePolicy", default)]
evidenceLanePolicy; evidence_lane_policy; EvidenceLaneRuntimeConfig;
`);
  writeFixture(root, DEFAULT_FILES.runtimeDomains, `
evidence-lane-policy; evidenceLanePolicy; compiled-runtime-evidence-lane-policy.json;
`);
  writeFixture(root, DEFAULT_FILES.lispcEmit, `
evidence_lane_policy_runtime_config_json; evidence_lane_entry_json; evidence_lane_profile_json; evidenceLanePolicy; find_child root "evidence-lane-policy";
`);
  writeFixture(root, DEFAULT_FILES.evidenceMigration, `
CREATE TABLE IF NOT EXISTS evidence_items;
CREATE TABLE IF NOT EXISTS context_gather_runs;
CREATE TABLE IF NOT EXISTS conversation_episodes;
CREATE TABLE IF NOT EXISTS conversation_fact_extracts;
CREATE TABLE IF NOT EXISTS conversation_duplicate_groups;
CREATE TABLE IF NOT EXISTS skill_evidence_items;
'runtime_truth'; 'project_ssot'; 'reviewed_kb'; 'active_board'; 'skill_evidence'; 'conversation_audit'; 'cold_archive'; 'support_refs';
`);
  writeFixture(root, DEFAULT_FILES.contextGather, `
SourceProfile; source_profile; source_selection; include_credentials; include_raw_sources; persist_read_model; persistReadModel; context_gather_persist_read_model;
include_board; include_conversations; conversation_time_range;
evidence_lanes; evidence_items; support_catalog; deployment_closure; deployment_closure_policy; authority_order; noise_diagnostics; context_noise_metrics; build_support_catalog; build_evidence_items; build_deployment_closure_support; persist_evidence_lane_projection; record_context_gather_run; upsert_evidence_items; runtime_truth; project_ssot; reviewed_kb; active_board; skill_evidence; conversation_audit; cold_archive; support_refs; context_pack_artifact_payload;
load_compiled_project_universe; compiled_service_runtime_payload_for_project; supportCatalog; compiled_deployment_policy_for_service;
credential_lane_opt_in; selection.include_credentials; selection.include_raw_sources; raw_sources_omitted;
"board_tasks"; "conversation_logs"; "credential_refs"; "mission_board_query"; "mission_conversation_query"; "scope": "active"; "time_range"; last_30d;
`);
  writeFixture(root, DEFAULT_FILES.mcpContextGather, `
runtime_truth; project_ssot; reviewed_kb; active_board; support_refs; skill_evidence; conversation_audit; cold_archive; evidence_lanes; evidence_items; support_catalog; source_profile; sourceProfile; intent_default; deploy_ops; conversation_audit; full_debug;
include_credentials; includeCredentials; include_raw_sources; includeRawSources;
persist; persist_read_model; persistReadModel;
include_board; include_conversations; conversation_time_range;
`);
  writeFixture(root, DEFAULT_FILES.toolDirectory, `
mission_context_gather + mission_conversation_* + mission_timeline + mission_audit;
"mission_context_gather"; "grounding"; "intent";
`);
  writeFixture(root, DEFAULT_FILES.kbArgs, `
pub(super) struct KBRememberArgs;
pub(super) struct KBKeyArgs;
pub(super) struct KBUpdateArgs;
pub(super) struct KBSearchArgs;
pub(super) exclude_category: Option<Value>;
pub(super) struct KBListArgs;
pub(super) struct KBImportArgs;
pub(super) struct KBDiscoverArgs;
pub(super) struct KBGCArgs;
pub(super) struct KBBatchRememberArgs;
pub(super) struct KBReviewArgs {
  pub(super) include_archived: bool,
  pub(super) state_filter: Option<String>,
}
lenient::option_i64;
fn default_list_limit() {}
`);
  writeFixture(root, DEFAULT_FILES.kbQuality, `
pub(super) fn check_content_quality() {
  architecture:summary; summary 过长; summary 为空; test write; batch-; stack trace; RUST_BACKTRACE; detail 过长;
}`);
  writeFixture(root, DEFAULT_FILES.kbRemember, `
pub(super) async fn handle_kb_remember() {
  KBRememberArgs; check_content_quality(); KBRememberInput; EmbeddingTask::ProcessKBEntry; consolidated_from; kb_add_edge(); kb_add_ast_link(); KBBatchMutated; detect_kb_conflicts(); write_duplicate_review_artifact(); KnowledgeReviewInput; needs-human; kb_adjust_confidence(); contradicts;
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
  Key is archived by KB review overlay;
}
pub(super) async fn handle_kb_list() {
  KBListArgs; kb_list_paginated(); "compact": true;
}
fn review_state_hidden() {}
async fn filter_entries_by_review() {
  kb_review_current_for_ids; kb_review_get_by_key; include_archived; state_filter; "unreviewed";
}
fn is_sensitive_retrieval_intent() {}
fn suppress_for_sensitive_retrieval() {
  "architecture:module";
}
fn parse_excluded_categories() {}
fn category_is_excluded() {}
exclude_category;
`);
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
  writeFixture(root, DEFAULT_FILES.kbReview, `
pub(super) async fn handle_kb_review() {
  KnowledgeReviewInput; VALID_REVIEW_STATES;
  "active"; "superseded-by-lisp"; "superseded-by-code"; "historical-evidence";
  "duplicate"; "wrong-or-stale"; "delete-candidate"; "needs-human";
  "upsert"; "get"; "stats";
  kb_review_upsert(); kb_review_get_by_key(); kb_review_current_for_ids(); kb_review_stats();
  resolve_knowledge_id; non_destructive;
}`);
  writeFixture(root, DEFAULT_FILES.kbReviewMigration, `
CREATE TABLE IF NOT EXISTS knowledge_review_state (
  id TEXT PRIMARY KEY,
  knowledge_id TEXT NOT NULL REFERENCES knowledge(id) ON DELETE CASCADE,
  state TEXT NOT NULL CHECK (
    state IN ('active', 'superseded-by-lisp', 'superseded-by-code', 'historical-evidence', 'duplicate', 'wrong-or-stale', 'delete-candidate', 'needs-human')
  ),
  batch_id TEXT NOT NULL,
  reviewer TEXT NOT NULL,
  rationale TEXT NOT NULL,
  evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
  is_current BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_review_state_current ON knowledge_review_state (knowledge_id) WHERE is_current;
`);
  writeFixture(root, DEFAULT_FILES.kbReviewTypes, `
pub struct KnowledgeReviewState {
  pub knowledge_id: String,
  pub state: String,
  pub batch_id: String,
  pub reviewer: String,
  pub rationale: String,
  pub evidence_refs: serde_json::Value,
  pub superseded_by: Option<String>,
  pub confidence: f64,
  pub is_current: bool,
}
pub struct KnowledgeReviewInput {
  pub knowledge_id: String,
  pub state: String,
  pub batch_id: String,
  pub reviewer: String,
  pub rationale: String,
  pub evidence_refs: serde_json::Value,
  pub superseded_by: Option<String>,
  pub confidence: f64,
  pub is_current: bool,
}
`);
  writeFixture(root, DEFAULT_FILES.kbReviewTraits, `
KnowledgeReviewInput; KnowledgeReviewState;
async fn kb_review_upsert() {}
async fn kb_review_current_for_ids() {}
async fn kb_review_get_by_key() {}
async fn kb_review_stats() {}
`);
  writeFixture(root, DEFAULT_FILES.kbReviewPg, `
async fn kb_review_upsert() {
  INSERT INTO knowledge_review_state;
  SET is_current = FALSE;
}
async fn kb_review_current_for_ids() {
  WHERE is_current = TRUE AND knowledge_id = ANY($1);
}
async fn kb_review_get_by_key() {
  JOIN knowledge k ON k.id = r.knowledge_id;
}
async fn kb_review_stats() {
  GROUP BY state;
}
SAME_SESSION_FUZZY_MERGE_THRESHOLD;
merge_detail_for_dedupe;
same_source_session;
_dedupe_merge_events;
`);
	  writeFixture(root, DEFAULT_FILES.memory, `
	MemoryKbRuntimeConfig; load_memory_kb_config; V3_BLUEPRINT_CONFIG_ERROR; pending_message_limit; tool_result_preview_chars; assistant_preview_chars; get_pending_realtime_messages_with_limit(pending_msg_limit); MAX_PENDING_BATCH_REPLAYS; classify_memory_input_noise; deployment-monitor; deployment-event-response; xjp_build_wait; xjp_deploy_watch; build_started; agent_update_failed; reported_digest_missing; runtime-report; worker-instruction; provider-preamble; inputSkipDiagnostics; inputFilter; mark_pending_batch_served; pending_payload; MEMORY_PENDING_ALREADY_SERVED; ToolResult::structured_error;
	`);
	  writeFixture(root, DEFAULT_FILES.learningMod, `
	LearningEngineRuntimeConfig; decision_harvest_interval_secs; cooccurrence_refresh_interval_secs; V3 learning-engine-policy unavailable;
	`);
	  writeFixture(root, DEFAULT_FILES.learningExtraction, `
	LearningEngineRuntimeConfig; load_learning_engine_config; realtime_extraction_timeout_ms; try_claim_extraction_probe; release_extraction_probe; should_skip_realtime_empty_backoff; should_skip_memory_due_to_token_spend_guard; token_spend_guard_window_secs; token_spend_guard_soft_limit; token_stats(None, Some(slot_id); CtlDomain::Memory; should_skip_deep_analysis_zero_output_fuse; record_deep_analysis_completion; deep_analysis_zero_output_fuse_threshold; record_realtime_empty_probe; reset_realtime_empty_backoff; another extraction probe already claimed the lane; kb_consolidation_interval_secs; kb_auto_gc_interval_secs; kb_reflection_interval_secs; kb_reflection_utility_threshold; kb_reflection_max_tokens;
	`);
	  writeFixture(root, DEFAULT_FILES.pgConversation, `
	SELECT COUNT(*) FROM conversations c WHERE EXISTS (SELECT 1 FROM conversation_messages m LIMIT 1);
	SELECT * FROM conversations c CROSS JOIN LATERAL (SELECT * FROM conversation_messages m LIMIT 15) m;
	WITH candidate AS MATERIALIZED (SELECT * FROM conversation_messages m LIMIT 2000) SELECT * FROM candidate;
	SELECT 1 FROM conversation_messages m ORDER BY m.id ASC OFFSET 99 LIMIT 1;
	slot_id NOT LIKE 'slot-memory%'; slot_id NOT LIKE 'slot-diagnosis%'; slot_id NOT LIKE 'agent-%';
	`);
	  writeFixture(root, DEFAULT_FILES.learningDecision, `
	LearningEngineRuntimeConfig; decision_tier3_timeout_ms;
	`);
	  writeFixture(root, DEFAULT_FILES.learningTimeline, `
	LearningEngineRuntimeConfig; UpsertTaskContractCommand; timeline_insight_runtime_metadata; "control_state": "task_contracts"; "sandbox_profile": "system-learning-review"; llm_gate::is_disabled; LlmProvider::Gemini; Timeline Analyst: skipped because Gemini gate is closed; last_timeline_analysis_at; timeline_analysis_interval_secs; timeline_window_arg; timeline_error_limit; timeline_llm_sample_limit; timeline_slow_threshold_ms;
	`);
	  writeFixture(root, DEFAULT_FILES.eventProjection, `
	conditions.push(format!("ts >= \${}::timestamptz", idx));
	conditions.push(format!("ts <= \${}::timestamptz", idx));
	WHERE ts >= $1::timestamptz AND ts <= $2::timestamptz
	`);
	  writeFixture(root, DEFAULT_FILES.learningIdle, `
	LearningEngineRuntimeConfig; UpsertTaskContractCommand; idle_exploration_runtime_metadata; "control_state": "task_contracts"; "sandbox_profile": "system-learning-review"; auto_execute: Some(false); idle_explore_interval_secs;
	`);
	  writeFixture(root, DEFAULT_FILES.experienceHarvester, `
	UpsertTaskContractCommand; skill_synthesis_runtime_metadata; "control_state": "task_contracts"; "sandbox_profile": "system-learning-review"; auto_execute: Some(false); skill_synthesis_metadata_declares_task_contract_authority;
	`);
	  writeFixture(root, DEFAULT_FILES.strategyWorker, `
	UpsertTaskContractCommand; strategy_skill_review_runtime_metadata; strategy_drift_review_runtime_metadata; "control_state": "task_contracts"; "sandbox_profile": "system-learning-review"; auto_execute: Some(false); strategy_skill_metadata_declares_task_contract_authority;
	`);
	  writeFixture(root, DEFAULT_FILES.learningHistorical, `
	LearningEngineRuntimeConfig; habit_scan_interval_secs; habit_scan_batch_size; habit_scan_timeout_ms;
	`);
	  writeFixture(root, DEFAULT_FILES.mcpKb, `
"mission_kb_query"; "mission_kb_remember"; "mission_kb_mutate"; "mission_kb_review"; "mission_kb_ops"; "mission_beacon"; "mission_code_search";
`);
  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content.trimStart());
}

main();
