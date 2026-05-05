#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs [--json]

Checks that V3 and code agree on CLI conversation ingestion sources:
  - canonical sources are claude_code, gemini_cli, and codex_cli.
  - each source has a watcher/reconcile/worker path into conversation storage.
  - source aliases are explicit, not accidental string drift.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  mcpDefs: '.missiond/intent-mcp-defs.lisp',
  ingestionRouter: 'crates/missiond-daemon/src/infra/ingestion_router.rs',
  messageHandler: 'crates/missiond-daemon/src/infra/message_handler.rs',
  eventsSync: 'crates/missiond-daemon/src/events_sync.rs',
  reconcileWorker: 'crates/missiond-daemon/src/workers/local/reconcile_worker.rs',
  conversationQuery: 'crates/missiond-daemon/src/handlers/comm/conversation/query.rs',
  conversationRouter: 'crates/missiond-daemon/src/handlers/comm/conversation/router.rs',
  conversationMaintenance: 'crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs',
  mcpConversation: 'crates/missiond-mcp/src/tools/comm/conversation.rs',
  // BoardTask e1a5ac1f :: provider-aware classification helper. Lives
  // alongside the existing ConversationQuery filter type so all
  // conversation classification logic stays in one read path.
  conversationClassifier: 'crates/missiond-core/src/db/conversation_query.rs',
  taggerChunker: 'crates/missiond-daemon/src/workers/local/tagger_chunker.rs',
  slotHandler: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  slotEnv: 'crates/missiond-daemon/src/context/slot_env.rs',
  slotOrchestrator: 'crates/missiond-daemon/src/slot_orchestrator/mod.rs',
  slotSpawner: 'crates/missiond-daemon/src/slot_orchestrator/spawner.rs',
  ccController: 'crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs',
  geminiController: 'crates/missiond-daemon/src/slot_orchestrator/gemini_controller.rs',
  claudeWatcher: 'crates/missiond-core/src/cc_tasks/watcher.rs',
  geminiWatcher: 'crates/missiond-core/src/gemini_cli/watcher.rs',
  geminiParser: 'crates/missiond-core/src/gemini_cli/parser.rs',
  geminiReconcile: 'crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs',
  codexIngestion: 'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
  pgMessage: 'crates/missiond-core/src/db/pg/message.rs',
  pgConversation: 'crates/missiond-core/src/db/pg/conversation.rs',
  duplicateReport: 'scripts/report-codex-conversation-duplicates.mjs',
  claudeRoleAudit: 'scripts/report-claude-role-attribution.mjs',
  sourceMigration: 'crates/missiond-core/migrations/20260501000000_canonical_cli_sources.sql',
  sourceCleanupMigration: 'crates/missiond-core/migrations/20260501001000_clean_stale_cli_slot_sessions.sql',
  conversationTypes: 'crates/missiond-core/src/types/conversation.rs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }
  const diagnostics = checkFiles(process.cwd(), DEFAULT_FILES);
  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 CLI conversation ingestion isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 CLI conversation ingestion check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    try {
      sources[key] =
        key === 'blueprint'
          ? readBlueprintWithEvidenceSidecars(root, rel)
          : fs.readFileSync(path.join(root, rel), 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    '(cli-conversation-ingestion',
    '(source claude-code',
    ':canonical "claude_code"',
    '(source gemini-cli',
    ':canonical "gemini_cli"',
    '"~/.gemini/tmp/*/chats/*.jsonl"',
    '(source codex-cli',
    ':canonical "codex_cli"',
    ':legacy-aliases ["claude_cli" "pty_jsonl"]',
    'Conversation sources MUST be canonicalized before DB write',
    'mission_slots MUST reject or flag slot_sessions whose conversation source disagrees with the slot engine',
    'mission_slots MUST fall back to the latest real codex_cli conversation',
    'Codex CLI message ingestion MUST generate deterministic non-null message_uuid values',
    'Codex CLI background ingestion MUST persist rollout size/mtime/line watermarks',
    'the DB layer MUST adopt that existing row by setting message_uuid instead of inserting a new duplicate row',
    'mission_conversation_get MUST defensively coalesce duplicate rows',
    'mission_conversation_get MUST retrieve tail messages with the indexed (session_id,id) path',
    'Historical duplicate cleanup is dry-run/report-first',
    'Gemini background reconcile MUST use size/mtime companion watermarks',
    'top-level raw_role=user inside automated slot sessions normalizes to worker_user',
    'raw_role is preserved for audit',
    'scripts/report-claude-role-attribution.mjs',
    'node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs',
    // BoardTask e1a5ac1f :: provider-aware classification authority.
    'classify_conversation_type',
    'background-ingested Codex threads classify as codex_chat',
    'audit_classification',
    'audit_historical_classification',
    'mission_conversation_query(action=audit_classification)',
    'mission_conversation_query(action=backfill_classification, apply=true)',
    'mission_conversation_query(action=turn_backfill, sessionId=...)',
    'codex_user_without_slot',
    'codex_raw_role_missing',
    'claude_worker_prompt_signature',
    'rebuild_session_turns',
  ]);

  requireAll(diagnostics, files.ingestionRouter, sources.ingestionRouter, [
    'Origin CLI: "claude_code", "gemini_cli", "codex_cli"',
    '"claude_cli" | "pty_jsonl" => "claude_code"',
    'canonical_event_source == "claude_code" || canonical_event_source == "gemini_cli"',
    'let source = canonical_event_source.to_string();',
  ]);

  rejectAll(diagnostics, files.ingestionRouter, sources.ingestionRouter, [
    '"pty_jsonl".to_string()',
  ]);

  requireAll(diagnostics, files.slotEnv, sources.slotEnv, [
    'updated.source = "claude_code".to_string();',
    '&updated.source',
  ]);
  rejectAll(diagnostics, files.slotEnv, sources.slotEnv, [
    'updated.source = "pty_jsonl".to_string();',
  ]);

  requireAll(diagnostics, files.slotOrchestrator, sources.slotOrchestrator, [
    'canonical_source_for_engine',
    'CliEngine::ClaudeCode => "claude_code"',
    'CliEngine::Gemini => "gemini_cli"',
    'CliEngine::Codex => "codex_cli"',
    'chat_type_for_source',
    'source: &str',
    'session_id,\n        source',
    'source: source.to_string()',
  ]);
  rejectAll(diagnostics, files.slotOrchestrator, sources.slotOrchestrator, [
    'session_id, "pty_jsonl"',
    'source: "pty_jsonl".to_string()',
  ]);

  requireAll(diagnostics, files.slotSpawner, sources.slotSpawner, [
    'missiond_core::CliEngine::Gemini | missiond_core::CliEngine::Codex',
    'format!("pty-{}", pty_slot.id)',
    'canonical_source_for_engine(pty_slot.engine)',
    'register_slot_session(',
  ]);

  requireAll(diagnostics, files.ccController, sources.ccController, [
    'register_slot_session(',
    '"claude_code"',
  ]);

  requireAll(diagnostics, files.geminiController, sources.geminiController, [
    'register_slot_session(',
    '"gemini_cli"',
  ]);

  requireAll(diagnostics, files.messageHandler, sources.messageHandler, [
    'canonical_source_for_engine(engine).to_string()',
    'chat_type_for_source(&source)',
    'updated.source = source.clone();',
    'normalize_claude_message_role(',
    'worker_user',
  ]);
  rejectAll(diagnostics, files.messageHandler, sources.messageHandler, [
    'source: "pty".to_string()',
  ]);

  requireAll(diagnostics, files.slotHandler, sources.slotHandler, [
    'conversation_source_matches_engine',
    'latest_provider_conversation_for_slot',
    'conversation_matches_slot_project',
    'providerConversationId',
    'pty-slot-',
    'canonical_source_for_engine',
    'latestConversationMismatch',
    'CliEngine::ClaudeCode => "claude_code"',
    'CliEngine::Gemini => "gemini_cli"',
    'CliEngine::Codex => "codex_cli"',
  ]);

  requireAll(diagnostics, files.claudeWatcher, sources.claudeWatcher, [
    'source: String',
    '"claude_code"',
  ]);

  requireAll(diagnostics, files.eventsSync, sources.eventsSync, [
    'pub fn normalize_claude_message_role',
    '"worker_user".to_string()',
    'raw_role: Some(msg.message.role.clone())',
    'role_normalization_tests',
    'worker_user_only_in_automated_slot_session',
  ]);

  requireAll(diagnostics, files.reconcileWorker, sources.reconcileWorker, [
    'normalize_claude_message_role(',
    'raw_role: Some(msg.message.role.clone())',
  ]);

  requireAll(diagnostics, files.conversationQuery, sources.conversationQuery, [
    '"rawRole": m.raw_role',
    'm.role == "worker_user"',
    'slot_id_for_display.clone()',
  ]);

  requireAll(diagnostics, files.conversationRouter, sources.conversationRouter, [
    '"audit_classification" => "mission_conversation_classification_audit"',
    '"backfill_classification" => "mission_conversation_classification_backfill"',
    '"turn_backfill" => "mission_conversation_turn_backfill"',
  ]);

  requireAll(diagnostics, files.conversationMaintenance, sources.conversationMaintenance, [
    'classification_audit_rows',
    'audit_historical_classification',
    'set_conversation_type',
    'rebuild_turns_for_session',
    'mission_conversation_classification_audit',
    'mission_conversation_classification_backfill',
    'mission_conversation_turn_backfill',
  ]);

  requireAll(diagnostics, files.mcpConversation, sources.mcpConversation, [
    '"audit_classification"',
    '"backfill_classification"',
    '"turn_backfill"',
    '"minConfidence"',
    '"rebuildTurns"',
  ]);

  requireAll(diagnostics, files.mcpDefs, sources.mcpDefs, [
    'audit_classification',
    'backfill_classification',
    'turn_backfill',
    'minConfidence',
    'rebuildTurns',
  ]);

  requireAll(diagnostics, files.taggerChunker, sources.taggerChunker, [
    'worker_user',
    '"user" | "worker_user" | "agent_user"',
  ]);

  requireAll(diagnostics, files.geminiWatcher, sources.geminiWatcher, [
    '*.{json,jsonl}',
    'Some("json" | "jsonl")',
    'source: "gemini_cli"',
    'WatcherEvent::NewMessages',
  ]);

  requireAll(diagnostics, files.geminiParser, sources.geminiParser, [
    'parse_jsonl_session',
    'sessionId',
    '$set',
    'Some("json" | "jsonl")',
    'test_parse_jsonl_session',
  ]);

  requireAll(diagnostics, files.geminiReconcile, sources.geminiReconcile, [
    '*.{json,jsonl}',
    'source: "gemini_cli".to_string()',
    'chat_type: Some("gemini_cli".to_string())',
    'SIZE_WATERMARK_PREFIX',
    'MTIME_WATERMARK_PREFIX',
    'can_skip_without_parse',
    'force_full_scan',
    'run_gemini_reconciliation(&state, false)',
    'run_gemini_reconciliation(state, true)',
  ]);

  requireAll(diagnostics, files.codexIngestion, sources.codexIngestion, [
    'source: "codex_cli".to_string()',
    'chat_type: Some("codex_cli".to_string())',
    'message_uuid: Some(codex_message_uuid(&thread.id, m))',
    'line_no',
    'source_event_hash',
    'Sha256',
    'CODEX_SIZE_WATERMARK_PREFIX',
    'CODEX_MTIME_WATERMARK_PREFIX',
    'CODEX_LINE_WATERMARK_PREFIX',
    'REPARSE_OVERLAP_LINES',
    'persisted_codex_watermark_matches',
    'persist_codex_file_watermarks',
    'skip_before_line',
    'codex_message_uuid_is_non_null_and_stable',
    // BoardTask e1a5ac1f :: provider-aware classification + raw_role
    // preservation. Codex ingestion must call the new classifier
    // (no more hardcoded `conversation_type: "user"`) and forward the
    // provider role into `raw_role` so downstream audits can reason
    // about turn segmentation without re-parsing JSONL.
    'classify_conversation_type',
    'raw_role: Some(m.role.clone())',
    'get_running_slot_task',
    'codex_background_thread_classifies_as_codex_chat_not_user',
    'codex_slot_thread_classifies_as_worker',
    'audit_passes_post_fix_codex_background_row',
  ]);
  rejectAll(diagnostics, files.codexIngestion, sources.codexIngestion, [
    // Pre-fix patterns that masked Codex worker traffic as anonymous
    // human user input. A regression here re-routes Codex chats into
    // the human Logs tab and silently drops provider metadata.
    'conversation_type: "user".to_string()',
    'raw_role: None,',
  ]);

  requireAll(
    diagnostics,
    files.conversationClassifier,
    sources.conversationClassifier,
    [
      'pub fn classify_conversation_type',
      'if source == "codex_cli"',
      '"codex_chat".to_string()',
      '"worker".to_string()',
      'pub fn audit_classification',
      'pub fn audit_historical_classification',
      'pub struct HistoricalClassificationFinding',
      'pub struct HistoricalClassificationInput',
      'claude_worker_prompt_signature',
      'looks_like_historical_worker_prompt',
      'pub struct ClassificationAuditFinding',
      'pub struct ClassificationAuditInput',
      'codex_user_without_slot',
      'codex_slot_not_worker',
      'worker_loses_slot_linkage',
      'codex_raw_role_missing',
      // Pin the audit/test contract: the classifier is exercised
      // through unit tests so a regression cannot land Codex traffic
      // back in the human Logs tab without breaking CI.
      'codex_background_session_without_slot_classifies_as_codex_chat',
      'codex_worker_with_slot_classifies_as_worker',
      'audit_flags_codex_user_without_slot_as_misclassification',
      'audit_flags_worker_row_that_lost_its_slot_linkage',
      'historical_audit_promotes_boardtask_prompt_to_worker',
      'historical_audit_keeps_human_question_as_user',
    ],
  );

  requireAll(diagnostics, files.pgConversation, sources.pgConversation, [
    'async fn set_conversation_type',
    'DELETE FROM conversation_turns WHERE session_id = $1',
    'async fn clear_conversation_turns',
  ]);

  requireAll(diagnostics, files.taggerChunker, sources.taggerChunker, [
    'pub(crate) async fn rebuild_session_turns',
    'clear_conversation_turns(session_id)',
  ]);

  requireAll(diagnostics, files.pgMessage, sources.pgMessage, [
    'coalesce_duplicate_messages',
    'adopt_existing_message_uuid',
    'UPDATE conversation_messages',
    'AND NOT EXISTS',
    'ORDER BY id ASC LIMIT $3',
    'ORDER BY id DESC LIMIT $2',
    'NULL::BIGINT AS seq',
    'fallback:{}\\u{1f}{}\\u{1f}{}\\u{1f}{}\\u{1f}{}',
    'msg.seq = Some((idx + 1) as i64)',
    'coalesce_duplicate_messages_prefers_uuid_and_resets_seq',
    'coalesce_duplicate_messages_falls_back_when_uuid_is_missing',
  ]);
  rejectAll(diagnostics, files.pgMessage, sources.pgMessage, [
    'ROW_NUMBER() OVER (PARTITION BY session_id',
  ]);

  requireAll(diagnostics, files.duplicateReport, sources.duplicateReport, [
    "mode: 'dry-run'",
    'duplicateIds',
    'keepId',
    'mission_conversation_query',
  ]);

  requireAll(diagnostics, files.claudeRoleAudit, sources.claudeRoleAudit, [
    "mode: 'dry-run'",
    'worker_user',
    'systemWorkerPromptSuspects',
    'mission_conversation_query',
  ]);

  requireAll(diagnostics, files.pgConversation, sources.pgConversation, [
    'source = $4',
    'chat_type = COALESCE($17, conversations.chat_type)',
    "VALUES ($1, $2, 'claude_code'",
    "source = 'claude_code'",
  ]);
  rejectAll(diagnostics, files.pgConversation, sources.pgConversation, [
    "VALUES ($1, $2, 'claude_cli'",
  ]);

  requireAll(diagnostics, files.sourceMigration, sources.sourceMigration, [
    "ALTER COLUMN source SET DEFAULT 'claude_code'",
    "source IN ('claude_cli', 'pty_jsonl')",
    "SET source = 'gemini_cli'",
    "conversation_type = 'gemini_chat'",
  ]);

  requireAll(diagnostics, files.sourceCleanupMigration, sources.sourceCleanupMigration, [
    'DELETE FROM slot_sessions',
    "ss.slot_id ILIKE '%gemini%' AND c.source <> 'gemini_cli'",
    "ss.slot_id ILIKE '%codex%' AND c.source <> 'codex_cli'",
    "c.source <> 'claude_code'",
  ]);

  requireAll(diagnostics, files.conversationTypes, sources.conversationTypes, [
    'source',
    'canonical: "claude_code" | "gemini_cli" | "codex_cli"',
    'chat_type',
    'conversation_type',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push({ file, message: `missing ${needle}` });
  }
}

function rejectAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) diagnostics.push({ file, message: `forbidden ${needle}` });
  }
}

main();
