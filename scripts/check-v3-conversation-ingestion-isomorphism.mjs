#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-conversation-ingestion-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 conversation-ingestion Lisp/code isomorphism contract:
  - conversation public runtime is split into facade/router/query/events/maintenance.
  - timeline and retrospective remain explicit adapters under the same surface.
  - MCP public tools and daemon dispatch expose the consolidated V3 entries.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  dispatcher: 'crates/missiond-daemon/src/handlers/mod.rs',
  v3Runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  facade: 'crates/missiond-daemon/src/handlers/comm/conversation.rs',
  router: 'crates/missiond-daemon/src/handlers/comm/conversation/router.rs',
  query: 'crates/missiond-daemon/src/handlers/comm/conversation/query.rs',
  events: 'crates/missiond-daemon/src/handlers/comm/conversation/events.rs',
  maintenance: 'crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs',
  timeline: 'crates/missiond-daemon/src/handlers/comm/timeline.rs',
  retrospective: 'crates/missiond-daemon/src/handlers/comm/retrospective.rs',
  contextPipeline: 'crates/missiond-daemon/src/context/context_pipeline.rs',
  visionWorker: 'crates/missiond-daemon/src/workers/codex/vision_worker.rs',
  codexCli: 'crates/missiond-daemon/src/llm/codex_cli.rs',
  pgConversation: 'crates/missiond-core/src/db/pg/conversation.rs',
  mcpConversation: 'crates/missiond-mcp/src/tools/comm/conversation.rs',
  mcpTimeline: 'crates/missiond-mcp/src/tools/comm/timeline.rs',
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
    console.log('v3 conversation-ingestion Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 conversation-ingestion Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
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
    'conversation-ingestion',
    '(v2-item conversation-ingestion',
    ':status runtime-projected',
    '(conversation-ingestion-policy',
    ':conversation-get-tail-default 50',
    ':conversation-search-default-limit 10',
    ':message-search-default-limit 20',
    ':conversation-events-default-limit 100',
    ':agent-trajectory-default-limit 200',
    ':timeline-query-default-limit 50',
    ':timeline-query-max-limit 200',
    ':intent-router-model "claude-opus-4.6"',
    ':intent-router-timeout-ms 10000',
    ':vision-codex-binary "codex"',
    ':vision-codex-model "gpt-5.4"',
    ':vision-codex-idle-timeout-secs 120',
    ':vision-codex-absolute-timeout-secs 300',
    '(tool-group conversation-ingestion-tools',
    '(surface conversation-ingestion',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/handlers/comm/conversation.rs',
    'crates/missiond-daemon/src/handlers/comm/conversation/router.rs',
    'crates/missiond-daemon/src/handlers/comm/conversation/query.rs',
    'crates/missiond-daemon/src/handlers/comm/conversation/events.rs',
    'crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs',
    'crates/missiond-daemon/src/handlers/comm/timeline.rs',
    'crates/missiond-daemon/src/handlers/comm/retrospective.rs',
    'crates/missiond-daemon/src/context/context_pipeline.rs',
    'crates/missiond-daemon/src/workers/codex/vision_worker.rs',
    'crates/missiond-daemon/src/llm/codex_cli.rs',
    'crates/missiond-core/src/db/pg/conversation.rs',
    'crates/missiond-mcp/src/tools/comm/conversation.rs',
    'crates/missiond-mcp/src/tools/comm/timeline.rs',
    'scripts/check-v3-conversation-ingestion-isomorphism.mjs',
    'conversation-ingestion-policy read-model default and max limits',
    'UserPromptSubmit context prefetch intent router model and timeout MUST project from conversation-ingestion-policy',
    'Codex vision worker binary/model/idle timeout and CodexCli absolute timeout MUST project from conversation-ingestion-policy',
    'conversation.rs is the thin conversation-ingestion facade',
    'conversation/router.rs owns mission_conversation_query',
    'conversation/query.rs owns read-model query actions',
    'when mission_conversation_query list is scoped by taskId and conversationType is omitted, query all provider conversation rows',
    'message-anchored BoardTask id fallback',
    'compaction timeline reconstruction tolerates legacy NULL started_at/message_count rows',
    'conversation/events.rs owns analysis/event egress',
    'conversation/maintenance.rs owns embedding/reconcile work items',
    'timeline.rs owns mission_timeline',
    'retrospective.rs owns retrospective analysis, list, and backfill',
    'vision_worker.rs owns unprocessed image-message extraction through CodexCli',
    'node scripts/check-v3-conversation-ingestion-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'ConversationIngestionRuntimeConfig',
    'parse_conversation_ingestion_policy',
    'DEFAULT_CONVERSATION_GET_TAIL',
    'DEFAULT_CONVERSATION_SEARCH_LIMIT',
    'DEFAULT_MESSAGE_SEARCH_LIMIT',
    'DEFAULT_CONVERSATION_EVENTS_LIMIT',
    'DEFAULT_AGENT_TRAJECTORY_LIMIT',
    'DEFAULT_TIMELINE_QUERY_LIMIT',
    'MAX_TIMELINE_QUERY_LIMIT',
    'DEFAULT_TIMELINE_SEARCH_LIMIT',
    'MAX_TIMELINE_SEARCH_LIMIT',
    'DEFAULT_INTENT_ROUTER_MODEL',
    'DEFAULT_INTENT_ROUTER_TIMEOUT_MS',
    'DEFAULT_VISION_CODEX_BINARY',
    'DEFAULT_VISION_CODEX_MODEL',
    'DEFAULT_VISION_CODEX_IDLE_TIMEOUT_SECS',
    'DEFAULT_VISION_CODEX_ABSOLUTE_TIMEOUT_SECS',
    'intent_router_model',
    'intent_router_timeout',
    'vision_codex_binary',
    'vision_codex_model',
    'vision_codex_idle_timeout',
    'vision_codex_absolute_timeout',
    ':intent-router-model',
    ':intent-router-timeout-ms',
    ':vision-codex-binary',
    ':vision-codex-absolute-timeout-secs',
    'conversation-ingestion-policy',
  ]);

  requireAll(diagnostics, files.dispatcher, sources.dispatcher, [
    'mission_conversation_',
    'mission_retrospective_manage',
    'mission_embedding_ops',
    'mission_activity_report',
    'conversation::handle',
  ]);

  requireAll(diagnostics, files.facade, sources.facade, [
    'mod router;',
    'mod query;',
    'mod events;',
    'mod maintenance;',
    '"mission_conversation_query" => router::handle_conversation_query',
    '"mission_conversation_analyze" => router::handle_conversation_analyze',
    '"mission_retrospective_manage" => router::handle_retrospective_manage',
    'query::handle_query',
    'events::handle_events',
    'maintenance::handle_maintenance',
    'Unknown conversation tool',
  ]);

  requireAll(diagnostics, files.router, sources.router, [
    'handle_conversation_query',
    'handle_conversation_analyze',
    'handle_retrospective_manage',
    '"mission_conversation_list"',
    '"mission_conversation_search"',
    '"mission_conversation_events"',
    '"mission_retrospective"',
    '"mission_agent_trajectory"',
    '"mission_activity_report"',
    '"mission_retrospective_list"',
    '"mission_retrospective_backfill"',
  ]);

  requireAll(diagnostics, files.query, sources.query, [
    'handle_query',
    'ConversationIngestionRuntimeConfig',
    'load_conversation_config',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'conversation_get_tail_default',
    'conversation_search_default_limit',
    'message_search_default_limit',
    'context_before_default',
    'context_after_default',
    '"mission_token_stats"',
    '"mission_conversation_list"',
    '"mission_conversation_get"',
    '"mission_conversation_search"',
    '"mission_message_search"',
    '"mission_user_message_index"',
    '"mission_conversation_set_label"',
    '"mission_conversation_delete_label"',
    '"mission_context_around"',
    'hybrid_message_search',
    'search_conversation_sessions_fts_filtered',
    'get_messages_around',
  ]);

  requireAll(diagnostics, files.events, sources.events, [
    'handle_events',
    'ConversationIngestionRuntimeConfig',
    'load_conversation_config',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'conversation_events_default_limit',
    'agent_trajectory_default_limit',
    '"mission_conversation_events"',
    '"mission_agent_trajectory"',
    '"mission_conversation_message"',
    '"mission_activity_report"',
    'get_conversation_events',
    'get_agent_trajectory',
    'query_timeline_stats',
  ]);

  requireAll(diagnostics, files.maintenance, sources.maintenance, [
    'handle_maintenance',
    '"mission_trigger_backfill"',
    '"mission_habit_scan"',
    '"mission_embedding_stats"',
    '"mission_embedding_ops"',
    '"mission_conversation_reconcile"',
    'EmbeddingTask::BackfillAll',
    'EmbeddingTask::RunBackfillPhase',
    'reconcile_conversation_messages',
    'run_reconciliation_now',
  ]);

  requireAll(diagnostics, files.timeline, sources.timeline, [
    'mission_timeline',
    'ConversationIngestionRuntimeConfig',
    'load_conversation_config',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'timeline_query_limit',
    'timeline_search_limit',
    'mission_timeline_query',
    'mission_timeline_trace',
    'mission_timeline_stats',
    'mission_timeline_search',
  ]);

  requireAll(diagnostics, files.retrospective, sources.retrospective, [
    'mission_retrospective_list',
    'mission_retrospective_backfill',
    'run_analysis',
    'get_retrospective_meta',
    'list_retrospective_results',
    'retro_worker::backfill',
  ]);

  requireAll(diagnostics, files.contextPipeline, sources.contextPipeline, [
    'ConversationIngestionRuntimeConfig::load_for_current_dir',
    'config.intent_router_model.as_str()',
    'config.intent_router_timeout()',
    'V3_BLUEPRINT_CONFIG_ERROR',
    '"model": model',
    'timeout.as_millis()',
  ]);
  forbidAll(diagnostics, files.contextPipeline, sources.contextPipeline, [
    'const INTENT_MODEL',
    'INTENT_ROUTE_TIMEOUT_MS',
    '"claude-opus-4.6"',
  ]);

  requireAll(diagnostics, files.visionWorker, sources.visionWorker, [
    'ConversationIngestionRuntimeConfig::load_for_current_dir',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'vision_codex_binary.clone()',
    'vision_codex_model.clone()',
    'vision_codex_idle_timeout()',
    'with_conversation_ingestion_config(&conversation_config)',
    'Vision worker started (codex',
  ]);
  forbidAll(diagnostics, files.visionWorker, sources.visionWorker, [
    '"gpt-5.4".to_string()',
    'Duration::from_secs(120)',
    'Vision worker started (codex/gpt-5.4',
  ]);

  requireAll(diagnostics, files.codexCli, sources.codexCli, [
    'ConversationIngestionRuntimeConfig',
    'absolute_timeout',
    'with_conversation_ingestion_config',
    'config.vision_codex_absolute_timeout()',
    'absolute timeout ({}s)',
  ]);
  forbidAll(diagnostics, files.codexCli, sources.codexCli, [
    'const ABSOLUTE_TIMEOUT',
    'Duration::from_secs(300)',
  ]);

  requireAll(diagnostics, files.pgConversation, sources.pgConversation, [
    'task_scoped_type_clause',
    'None | Some("all") => String::new()',
    'task_scoped_query_without_type_includes_provider_conversations',
    'task_scoped_query_keeps_explicit_type_filters',
    "m.content ILIKE ('%' || $1 || '%')",
    "COALESCE(started_at, '') AS started_at",
    'COALESCE(message_count, 0) AS message_count',
  ]);

  requireAll(diagnostics, files.mcpConversation, sources.mcpConversation, [
    'ToolDefinition::new',
    '"mission_conversation_query"',
    '"mission_conversation_analyze"',
    '"mission_conversation_reconcile"',
    '"mission_retrospective_manage"',
    '"mission_embedding_ops"',
    '"retrospective"',
    '"trajectory"',
    '"activity"',
  ]);

  requireAll(diagnostics, files.mcpTimeline, sources.mcpTimeline, [
    'ToolDefinition::new',
    '"mission_timeline"',
    '"query"',
    '"trace"',
    '"stats"',
    '"search"',
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

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden text present: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-conversation-ingestion-'));
  for (const rel of Object.values(DEFAULT_FILES)) {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  }
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.blueprint),
    `
(missiond-blueprint
  (conversation-ingestion-policy
    :conversation-get-tail-default 50
    :conversation-search-default-limit 10
    :message-search-default-limit 20
    :context-before-default 3
    :context-after-default 5
    :conversation-events-default-limit 100
    :agent-trajectory-default-limit 200
    :timeline-query-default-limit 50
    :timeline-query-max-limit 200
    :timeline-search-default-limit 20
    :timeline-search-max-limit 100
    :intent-router-model "claude-opus-4.6"
    :intent-router-timeout-ms 10000
    :vision-codex-binary "codex"
    :vision-codex-model "gpt-5.4"
    :vision-codex-idle-timeout-secs 120
    :vision-codex-absolute-timeout-secs 300)
  (v2-convergence-map
    (v2-item conversation-ingestion :status runtime-projected))
  (public-surface-map
    (tool-group conversation-ingestion-tools :status code-aligned))
  (implementation-map
    (surface conversation-ingestion
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/comm/conversation.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation/router.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation/query.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation/events.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs"
             "crates/missiond-daemon/src/handlers/comm/timeline.rs"
             "crates/missiond-daemon/src/handlers/comm/retrospective.rs"
             "crates/missiond-daemon/src/context/context_pipeline.rs"
             "crates/missiond-daemon/src/workers/codex/vision_worker.rs"
             "crates/missiond-daemon/src/llm/codex_cli.rs"
             "crates/missiond-core/src/db/pg/conversation.rs"
             "crates/missiond-mcp/src/tools/comm/conversation.rs"
             "crates/missiond-mcp/src/tools/comm/timeline.rs"
             "scripts/check-v3-conversation-ingestion-isomorphism.mjs"]
      :note "conversation-ingestion-policy read-model default and max limits; UserPromptSubmit context prefetch intent router model and timeout MUST project from conversation-ingestion-policy; Codex vision worker binary/model/idle timeout and CodexCli absolute timeout MUST project from conversation-ingestion-policy; conversation.rs is the thin conversation-ingestion facade; conversation/router.rs owns mission_conversation_query; conversation/query.rs owns read-model query actions; when mission_conversation_query list is scoped by taskId and conversationType is omitted, query all provider conversation rows; conversation/events.rs owns analysis/event egress; conversation/maintenance.rs owns embedding/reconcile work items; timeline.rs owns mission_timeline; retrospective.rs owns retrospective analysis, list, and backfill; vision_worker.rs owns unprocessed image-message extraction through CodexCli."))
  (compression-contract
    :checks ["node scripts/check-v3-conversation-ingestion-isomorphism.mjs"]))`,
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.dispatcher),
    'mission_conversation_ mission_retrospective_manage mission_embedding_ops mission_activity_report conversation::handle',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.v3Runtime),
    'ConversationIngestionRuntimeConfig parse_conversation_ingestion_policy DEFAULT_CONVERSATION_GET_TAIL DEFAULT_CONVERSATION_SEARCH_LIMIT DEFAULT_MESSAGE_SEARCH_LIMIT DEFAULT_CONVERSATION_EVENTS_LIMIT DEFAULT_AGENT_TRAJECTORY_LIMIT DEFAULT_TIMELINE_QUERY_LIMIT MAX_TIMELINE_QUERY_LIMIT DEFAULT_TIMELINE_SEARCH_LIMIT MAX_TIMELINE_SEARCH_LIMIT DEFAULT_INTENT_ROUTER_MODEL DEFAULT_INTENT_ROUTER_TIMEOUT_MS DEFAULT_VISION_CODEX_BINARY DEFAULT_VISION_CODEX_MODEL DEFAULT_VISION_CODEX_IDLE_TIMEOUT_SECS DEFAULT_VISION_CODEX_ABSOLUTE_TIMEOUT_SECS intent_router_model intent_router_timeout vision_codex_binary vision_codex_model vision_codex_idle_timeout vision_codex_absolute_timeout :intent-router-model :intent-router-timeout-ms :vision-codex-binary :vision-codex-absolute-timeout-secs conversation-ingestion-policy',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.facade),
    'mod router; mod query; mod events; mod maintenance; "mission_conversation_query" => router::handle_conversation_query "mission_conversation_analyze" => router::handle_conversation_analyze "mission_retrospective_manage" => router::handle_retrospective_manage query::handle_query events::handle_events maintenance::handle_maintenance Unknown conversation tool',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.router),
    'handle_conversation_query handle_conversation_analyze handle_retrospective_manage "mission_conversation_list" "mission_conversation_search" "mission_conversation_events" "mission_retrospective" "mission_agent_trajectory" "mission_activity_report" "mission_retrospective_list" "mission_retrospective_backfill"',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.query),
    'handle_query ConversationIngestionRuntimeConfig load_conversation_config V3_BLUEPRINT_CONFIG_ERROR conversation_get_tail_default conversation_search_default_limit message_search_default_limit context_before_default context_after_default "mission_token_stats" "mission_conversation_list" "mission_conversation_get" "mission_conversation_search" "mission_message_search" "mission_user_message_index" "mission_conversation_set_label" "mission_conversation_delete_label" "mission_context_around" hybrid_message_search search_conversation_sessions_fts_filtered get_messages_around',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.events),
    'handle_events ConversationIngestionRuntimeConfig load_conversation_config V3_BLUEPRINT_CONFIG_ERROR conversation_events_default_limit agent_trajectory_default_limit "mission_conversation_events" "mission_agent_trajectory" "mission_conversation_message" "mission_activity_report" get_conversation_events get_agent_trajectory query_timeline_stats',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.maintenance),
    'handle_maintenance "mission_trigger_backfill" "mission_habit_scan" "mission_embedding_stats" "mission_embedding_ops" "mission_conversation_reconcile" EmbeddingTask::BackfillAll EmbeddingTask::RunBackfillPhase reconcile_conversation_messages run_reconciliation_now',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.timeline),
    'mission_timeline ConversationIngestionRuntimeConfig load_conversation_config V3_BLUEPRINT_CONFIG_ERROR timeline_query_limit timeline_search_limit mission_timeline_query mission_timeline_trace mission_timeline_stats mission_timeline_search',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.retrospective),
    'mission_retrospective_list mission_retrospective_backfill run_analysis get_retrospective_meta list_retrospective_results retro_worker::backfill',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.contextPipeline),
    'ConversationIngestionRuntimeConfig::load_for_current_dir config.intent_router_model.as_str() config.intent_router_timeout() V3_BLUEPRINT_CONFIG_ERROR "model": model timeout.as_millis()',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.visionWorker),
    'ConversationIngestionRuntimeConfig::load_for_current_dir V3_BLUEPRINT_CONFIG_ERROR vision_codex_binary.clone() vision_codex_model.clone() vision_codex_idle_timeout() with_conversation_ingestion_config(&conversation_config) Vision worker started (codex',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.codexCli),
    'ConversationIngestionRuntimeConfig absolute_timeout with_conversation_ingestion_config config.vision_codex_absolute_timeout() absolute timeout ({}s)',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.pgConversation),
    'task_scoped_type_clause None | Some("all") => String::new() task_scoped_query_without_type_includes_provider_conversations task_scoped_query_keeps_explicit_type_filters',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.mcpConversation),
    'ToolDefinition::new "mission_conversation_query" "mission_conversation_analyze" "mission_conversation_reconcile" "mission_retrospective_manage" "mission_embedding_ops" "retrospective" "trajectory" "activity"',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.mcpTimeline),
    'ToolDefinition::new "mission_timeline" "query" "trace" "stats" "search"',
  );
  return root;
}

main();
