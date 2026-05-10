#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-router-policy-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 router-policy Lisp/code isomorphism contract:
  - mission_router_chat public runtime is split into facade/chat/files/manage.
  - mission_plan router-policy dry-run remains advisory-only.
  - router-policy/backend-registry/dispatch-descriptor checker scripts stay pinned.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  facade: 'crates/missiond-daemon/src/handlers/comm/router_chat.rs',
  chat: 'crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs',
  files: 'crates/missiond-daemon/src/handlers/comm/router_chat/files.rs',
  manage: 'crates/missiond-daemon/src/handlers/comm/router_chat/manage.rs',
  v3Runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  embeddingWorker: 'crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs',
  geminiClient: 'crates/missiond-daemon/src/llm/gemini_client.rs',
  geminiCli: 'crates/missiond-daemon/src/llm/gemini_cli.rs',
  geminiDriver: 'crates/missiond-daemon/src/llm/gemini_driver.rs',
  geminiFileApi: 'crates/missiond-daemon/src/llm/gemini_file_api.rs',
  llmGateway: 'crates/missiond-daemon/src/llm/llm_gateway.rs',
  sonnetGateway: 'crates/missiond-daemon/src/llm/sonnet_gateway.rs',
  translationWorker: 'crates/missiond-daemon/src/workers/sonnet/translation_worker.rs',
  xjpRouterClient: 'crates/missiond-daemon/src/llm/xjp_router_client.rs',
  mcp: 'crates/missiond-mcp/src/tools/comm/router_chat.rs',
  planAdapter: 'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs',
  predicate: 'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs',
  readiness: 'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs',
  descriptor: 'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs',
  schemaParser: 'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs',
  policyChecker: 'scripts/check-router-policy.mjs',
  registryChecker: 'scripts/check-router-backend-registry.mjs',
  descriptorChecker: 'scripts/check-router-dispatch-descriptor.mjs',
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
    console.log('v3 router-policy Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 router-policy Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
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
    'router-policy',
    'router-runtime-policy',
    '(surface router-policy',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/handlers/comm/router_chat.rs',
    'crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs',
    'crates/missiond-daemon/src/handlers/comm/router_chat/files.rs',
    'crates/missiond-daemon/src/handlers/comm/router_chat/manage.rs',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/main.rs',
    'crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs',
    'crates/missiond-daemon/src/llm/gemini_client.rs',
    'crates/missiond-daemon/src/llm/gemini_cli.rs',
    'crates/missiond-daemon/src/llm/gemini_driver.rs',
    'crates/missiond-daemon/src/llm/gemini_file_api.rs',
    'crates/missiond-daemon/src/llm/llm_gateway.rs',
    'crates/missiond-daemon/src/llm/sonnet_gateway.rs',
    'crates/missiond-daemon/src/workers/sonnet/translation_worker.rs',
    'crates/missiond-daemon/src/llm/xjp_router_client.rs',
    'crates/missiond-mcp/src/tools/comm/router_chat.rs',
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs',
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs',
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs',
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs',
    'crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs',
    'scripts/check-router-policy.mjs',
    'scripts/check-router-backend-registry.mjs',
    'scripts/check-router-dispatch-descriptor.mjs',
    'scripts/check-v3-router-policy-isomorphism.mjs',
    'router_chat.rs is the thin router-policy facade',
    'router_chat/chat.rs owns mission_router_chat',
    'router_chat/files.rs owns attachment denylist and Gemini File API policy',
    'router_chat/manage.rs owns mission_router_chat_manage',
    'RouterRuntimeConfig',
    'mission_router_chat default model and max_tokens MUST project from router-runtime-policy',
    'mission_router_chat default idle_timeout MUST project from router-runtime-policy',
    'Flow daemon Gemini calls, stateless Sonnet calls, and queued SonnetGateway calls MUST project their model',
    'GeminiPtyDriver default slot model MUST project from router-runtime-policy flow-gemini-model',
    'Gemini CLI transport missing llm.yaml model MUST project from router-runtime-policy flow-gemini-model',
    'GeminiClient CLI mode MUST forward non-empty caller model to GeminiCli',
    'GeminiClient PTY/HTTP request queue timeouts MUST project from router-runtime-policy',
    'Gemini File API upload and poll timeouts MUST project from router-runtime-policy',
    'Gemini CLI absolute and tool-exec timeouts MUST project from router-runtime-policy',
    'Queued SonnetGateway quota throttle sleep MUST project from router-runtime-policy',
    'Translation worker message_translations.model MUST record the queued SonnetGateway model projected from router-runtime-policy',
    'GeminiClient request queue timeouts',
    'Gemini CLI absolute/tool-exec timeouts',
    'Gemini File API upload/poll timeouts',
    'queued Sonnet quota throttle',
    'xjp-router embedding client MUST project its missing timeout default from router-runtime-policy direct HTTP timeout',
    'BoardTask urgent/ops/docs-test-chore ANTHROPIC_MODEL overrides MUST project from router-runtime-policy',
    'xjp-router embedding timeout default',
    ':anthropic-urgent-model',
    ':anthropic-ops-model',
    ':anthropic-docs-test-chore-model',
    'plan/router_policy_dry_run.rs owns the advisory dry-run adapter',
    'dry_run_only/runtime_replacement/no_execution invariants',
    'node scripts/check-v3-router-policy-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.facade, sources.facade, [
    'mod chat;',
    'mod files;',
    'mod manage;',
    '"mission_router_chat" => chat::handle_chat(state, args).await',
    '"mission_router_chat_manage" => manage::handle_consolidated(state, args).await',
    '"mission_router_chat_history"',
    '"mission_router_chat_compress"',
    'manage::handle_legacy',
    'Unknown router_chat tool',
  ]);

  requireAll(diagnostics, files.chat, sources.chat, [
    'handle_chat',
    'apply_context_budget',
    'MAX_ROUTER_PAYLOAD_BYTES',
    'resolve_llm_credentials',
    'REQUEST_CALLER',
    'GeminiFileApi',
    'PreparedFile',
    'resolve_gemini_api_key',
    'is_file_denied',
    'FILE_MAX_SIZE_BINARY',
    'FILE_MAX_SIZE_TEXT',
    'RouterRuntimeConfig::load_for_current_dir',
    'router_config.default_chat_model',
    'router_config.chat_default_max_tokens',
    'router_config.file_chat_default_max_tokens',
    'router_config.router_chat_idle_timeout()',
    'router_chat_get_or_create',
    'router_chat_get_summary',
    'router_chat_load_active_history',
    'kb_list',
    'list_board_tasks',
    'send_with_timeout',
    'router_chat_append_messages',
    'finish_reason',
    'context_budget',
  ]);

  requireAll(diagnostics, files.files, sources.files, [
    'FILE_DENY_PATTERNS',
    'FILE_DENY_NAMES',
    'FILE_MAX_SIZE_TEXT',
    'FILE_MAX_SIZE_BINARY',
    'resolve_gemini_api_key',
    'is_file_denied',
    '/.ssh/',
    '.env',
    'credentials.json',
  ]);

  requireAll(diagnostics, files.manage, sources.manage, [
    'handle_consolidated',
    'handle_legacy',
    'mission_router_chat_history',
    'mission_router_chat_list',
    'mission_router_chat_stats',
    'mission_router_chat_clear',
    'mission_router_chat_delete',
    'mission_router_chat_delete_message',
    'mission_router_chat_restore',
    'mission_router_chat_compress',
    'router_chat_load_history',
    'router_chat_list',
    'router_chat_stats',
    'router_chat_clear',
    'router_chat_delete',
    'router_chat_delete_message',
    'router_chat_restore',
    'router_chat_update_summary',
    'REQUEST_CALLER',
    'router_chat_compress',
    'RouterRuntimeConfig::load_for_current_dir',
    'router_config.default_chat_model',
    'router_config.compress_model',
    'router_config.compress_channel',
    'router_config.compress_max_tokens',
    'router_config.compress_char_budget_chars',
  ]);

  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'pub(crate) struct RouterRuntimeConfig',
    'DEFAULT_ROUTER_CHAT_MODEL',
    'DEFAULT_ROUTER_FLOW_GEMINI_MODEL',
    'DEFAULT_ROUTER_STATELESS_SONNET_MODEL',
    'DEFAULT_ROUTER_QUEUED_SONNET_MODEL',
    'DEFAULT_ROUTER_CHAT_IDLE_TIMEOUT_SECS',
    'DEFAULT_ROUTER_GEMINI_PTY_QUEUE_TIMEOUT_SECS',
    'DEFAULT_ROUTER_GEMINI_HTTP_QUEUE_TIMEOUT_SECS',
    'DEFAULT_ROUTER_GEMINI_FILE_UPLOAD_TIMEOUT_SECS',
    'DEFAULT_ROUTER_GEMINI_FILE_POLL_TIMEOUT_SECS',
    'DEFAULT_ROUTER_GEMINI_CLI_ABSOLUTE_TIMEOUT_SECS',
    'DEFAULT_ROUTER_GEMINI_CLI_TOOL_EXEC_TIMEOUT_SECS',
    'DEFAULT_ROUTER_QUEUED_SONNET_QUOTA_THROTTLE_SECS',
    'gemini_pty_queue_timeout',
    'gemini_http_queue_timeout',
    'gemini_file_upload_timeout',
    'gemini_file_poll_timeout',
    'gemini_cli_absolute_timeout',
    'gemini_cli_tool_exec_timeout',
    'queued_sonnet_quota_throttle',
    'DEFAULT_ROUTER_ANTHROPIC_URGENT_MODEL',
    'DEFAULT_ROUTER_ANTHROPIC_OPS_MODEL',
    'DEFAULT_ROUTER_ANTHROPIC_DOCS_TEST_CHORE_MODEL',
    'anthropic_urgent_model',
    'anthropic_ops_model',
    'anthropic_docs_test_chore_model',
    'DEFAULT_ROUTER_COMPRESS_MODEL',
    'parse_router_runtime_policy',
    'find_form(source, "router-runtime-policy")',
    'direct_http_timeout',
    'router_chat_idle_timeout',
    ':router-chat-idle-timeout-secs',
    ':gemini-pty-queue-timeout-secs',
    ':gemini-http-queue-timeout-secs',
    ':gemini-file-upload-timeout-secs',
    ':gemini-file-poll-timeout-secs',
    ':gemini-cli-absolute-timeout-secs',
    ':gemini-cli-tool-exec-timeout-secs',
    ':queued-sonnet-quota-throttle-secs',
  ]);

  requireAll(diagnostics, files.main, sources.main, [
    'RouterRuntimeConfig::load_for_current_dir',
    'with_router_runtime_config(&router_runtime_config)',
    'cli_cfg.resolved_model(&router_runtime_config)',
    'cli_model',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);

  requireAll(diagnostics, files.embeddingWorker, sources.embeddingWorker, [
    'RouterRuntimeConfig',
    'default_model: Option<String>',
    'pub model: Option<String>',
    'resolved_model(&self, router_config: &RouterRuntimeConfig)',
    'router_config.flow_gemini_model.clone()',
  ]);
  forbidAll(diagnostics, files.embeddingWorker, sources.embeddingWorker, [
    '"gemini-3.1-pro-preview"',
    '"gpt-4o"',
  ]);

  requireAll(diagnostics, files.geminiClient, sources.geminiClient, [
    'RouterRuntimeConfig',
    'pty_queue_timeout',
    'http_queue_timeout',
    'with_router_runtime_config',
    'map(str::trim)',
    'filter(|model| !model.is_empty())',
    'config.gemini_pty_queue_timeout()',
    'config.gemini_http_queue_timeout()',
    'self.pty_queue_timeout',
    'self.http_queue_timeout',
  ]);
  forbidAll(diagnostics, files.geminiClient, sources.geminiClient, [
    'Duration::from_secs(30)',
    'Duration::from_secs(300)',
    '"gemini-3.1-pro-preview"',
  ]);

  requireAll(diagnostics, files.geminiCli, sources.geminiCli, [
    'RouterRuntimeConfig',
    'absolute_timeout',
    'tool_exec_timeout',
    'with_router_runtime_config',
    'config.gemini_cli_absolute_timeout()',
    'config.gemini_cli_tool_exec_timeout()',
    'absolute timeout ({}s)',
  ]);
  forbidAll(diagnostics, files.geminiCli, sources.geminiCli, [
    'const ABSOLUTE_TIMEOUT',
    'const TOOL_EXEC_TIMEOUT',
    'Duration::from_secs(900)',
    'Duration::from_secs(300)',
  ]);

  requireAll(diagnostics, files.geminiDriver, sources.geminiDriver, [
    'RouterRuntimeConfig',
    'RouterRuntimeConfig::load_for_project_root',
    'router_config.flow_gemini_model.as_str()',
    'model.unwrap_or(default_model)',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);
  forbidAll(diagnostics, files.geminiDriver, sources.geminiDriver, [
    'const GEMINI_MODEL',
    '"gemini-3.1-pro-preview"',
  ]);

  requireAll(diagnostics, files.geminiFileApi, sources.geminiFileApi, [
    'RouterRuntimeConfig',
    'new_with_router_runtime_config',
    'config.gemini_file_upload_timeout()',
    'config.gemini_file_poll_timeout()',
    'poll_timeout',
    'self.poll_timeout',
  ]);
  forbidAll(diagnostics, files.geminiFileApi, sources.geminiFileApi, [
    'const POLL_TIMEOUT',
    'Duration::from_secs(600)',
    'Duration::from_secs(300)',
  ]);

  requireAll(diagnostics, files.llmGateway, sources.llmGateway, [
    'RouterRuntimeConfig::load_for_current_dir',
    'router_config.flow_gemini_model',
    'router_config.chat_default_max_tokens',
    'router_config.stateless_sonnet_model',
    'router_config.anthropic_urgent_model',
    'router_config.anthropic_ops_model',
    'router_config.anthropic_docs_test_chore_model',
    'router_config.direct_http_timeout()',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);
  forbidAll(diagnostics, files.llmGateway, sources.llmGateway, [
    'claude-opus-4-6',
    'claude-sonnet-4-6',
    'claude-haiku-4-5-20251001',
  ]);

  requireAll(diagnostics, files.sonnetGateway, sources.sonnetGateway, [
    'RouterRuntimeConfig::load_for_current_dir',
    'queued_sonnet_model',
    'queued_sonnet_default_max_tokens',
    'model: Arc<str>',
    'pub(crate) fn model(&self) -> &str',
    'queued_sonnet_quota_throttle()',
    'quota_throttle_sleep',
    'config.direct_http_timeout()',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);
  forbidAll(diagnostics, files.sonnetGateway, sources.sonnetGateway, [
    'Duration::from_secs(30)',
    'throttling 30s',
  ]);

  requireAll(diagnostics, files.translationWorker, sources.translationWorker, [
    'sonnet.model().to_string()',
    'insert_translation(ctx.message_id, &translation, &model, duration_ms)',
  ]);
  forbidAll(diagnostics, files.translationWorker, sources.translationWorker, [
    '"MiniMax-M2.5-highspeed"',
  ]);

  requireAll(diagnostics, files.xjpRouterClient, sources.xjpRouterClient, [
    'RouterRuntimeConfig::load_for_current_dir',
    'RouterRuntimeConfig::default().direct_http_timeout()',
    'resolve_from_with_default_timeout',
    'router_config.direct_http_timeout()',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'timeout_secs',
    'unwrap_or(default_timeout)',
  ]);
  forbidAll(diagnostics, files.xjpRouterClient, sources.xjpRouterClient, [
    'DEFAULT_TIMEOUT_SECS',
    'timeout_secs.unwrap_or(120)',
    'Duration::from_secs(timeout_secs.unwrap_or',
  ]);

  requireAll(diagnostics, files.mcp, sources.mcp, [
    'ToolDefinition::new',
    '"mission_router_chat"',
    '"idle_timeout": {"type": "integer", "description": "CLI 模式空闲超时秒数", "default": 600}',
    '"mission_router_chat_manage"',
    '"history"',
    '"list"',
    '"delete"',
    '"clear"',
    '"delete_message"',
    '"restore"',
    '"stats"',
    '"compress"',
  ]);

  requireAll(diagnostics, files.planAdapter, sources.planAdapter, [
    'RouterPolicyMode',
    'parse_router_policy_mode',
    'attach_router_recommendation_block',
    'compute_recommendation',
    'DEFAULT_POLICY_PATH',
    'missiond.router-recommendation.v0',
    'applied=false',
    'dry_run',
    'runtime_replacement',
  ]);

  requireAll(diagnostics, files.predicate, sources.predicate, [
    'project_context',
    'evaluate_clause',
    'arg_string',
    'glob_match',
  ]);

  requireAll(diagnostics, files.readiness, sources.readiness, [
    'load_trace_index',
    'load_backend_registry',
    'attach_trace_index_fields',
    'attach_backend_readiness_fields',
    'RICH_TRACE_THRESHOLD',
    'BackendRegistryInfo',
  ]);

  requireAll(diagnostics, files.descriptor, sources.descriptor, [
    'attach_router_dispatch_descriptor',
    'dry_run_only',
    'runtime_replacement',
    'no_execution',
    'router_apply_eligible',
  ]);

  requireAll(diagnostics, files.schemaParser, sources.schemaParser, [
    'parse_router_policy',
    'parse_backend_registry',
    'router-policy',
    'router-backend-registry',
  ]);

  requireAll(diagnostics, files.policyChecker, sources.policyChecker, [
    'missiond.router-policy.v1',
    'dry-run-only true',
    'runtime-replacement false',
  ]);

  requireAll(diagnostics, files.registryChecker, sources.registryChecker, [
    'missiond.router-backend-registry.v1',
    'runtime_allowed',
    'readiness_status',
  ]);

  requireAll(diagnostics, files.descriptorChecker, sources.descriptorChecker, [
    'missiond.router-dispatch-descriptor.v1',
    'dry_run_only',
    'runtime_replacement',
    'no_execution',
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
      diagnostics.push({ file, message: `forbidden contract text is still present: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-router-policy-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
	  (implementation-map
	    (surface router-policy
	      :status "code-aligned"
	      :code ["crates/missiond-daemon/src/handlers/comm/router_chat.rs"
	             "crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs"
	             "crates/missiond-daemon/src/handlers/comm/router_chat/files.rs"
	             "crates/missiond-daemon/src/handlers/comm/router_chat/manage.rs"
	             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
	             "crates/missiond-daemon/src/main.rs"
	             "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
	             "crates/missiond-daemon/src/llm/gemini_client.rs"
	             "crates/missiond-daemon/src/llm/gemini_cli.rs"
	             "crates/missiond-daemon/src/llm/gemini_file_api.rs"
	             "crates/missiond-daemon/src/llm/llm_gateway.rs"
	             "crates/missiond-daemon/src/llm/sonnet_gateway.rs"
	             "crates/missiond-daemon/src/workers/sonnet/translation_worker.rs"
	             "crates/missiond-daemon/src/llm/xjp_router_client.rs"
	             "crates/missiond-mcp/src/tools/comm/router_chat.rs"
	             "crates/missiond-daemon/src/llm/gemini_driver.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs"
	             "scripts/check-router-policy.mjs"
	             "scripts/check-router-backend-registry.mjs"
	             "scripts/check-router-dispatch-descriptor.mjs"
	             "scripts/check-v3-router-policy-isomorphism.mjs"]
	      :note "router_chat.rs is the thin router-policy facade; router_chat/chat.rs owns mission_router_chat; router_chat/files.rs owns attachment denylist and Gemini File API policy; router_chat/manage.rs owns mission_router_chat_manage; RouterRuntimeConfig projects router-runtime-policy; mission_router_chat default model and max_tokens MUST project from router-runtime-policy; mission_router_chat default idle_timeout MUST project from router-runtime-policy; Flow daemon Gemini calls, stateless Sonnet calls, and queued SonnetGateway calls MUST project their model; GeminiPtyDriver default slot model MUST project from router-runtime-policy flow-gemini-model; Gemini CLI transport missing llm.yaml model MUST project from router-runtime-policy flow-gemini-model; GeminiClient CLI mode MUST forward non-empty caller model to GeminiCli; GeminiClient PTY/HTTP request queue timeouts MUST project from router-runtime-policy; Gemini File API upload and poll timeouts MUST project from router-runtime-policy; Gemini CLI absolute and tool-exec timeouts MUST project from router-runtime-policy; Queued SonnetGateway quota throttle sleep MUST project from router-runtime-policy; Translation worker message_translations.model MUST record the queued SonnetGateway model projected from router-runtime-policy; GeminiClient request queue timeouts; Gemini CLI absolute/tool-exec timeouts; Gemini File API upload/poll timeouts; queued Sonnet quota throttle; xjp-router embedding client MUST project its missing timeout default from router-runtime-policy direct HTTP timeout; xjp-router embedding timeout default; BoardTask urgent/ops/docs-test-chore ANTHROPIC_MODEL overrides MUST project from router-runtime-policy; plan/router_policy_dry_run.rs owns the advisory dry-run adapter and dry_run_only/runtime_replacement/no_execution invariants."))
	  (router-runtime-policy
	    :default-chat-model "gemini-3.1-pro"
	    :chat-default-max-tokens 16384
	    :file-chat-default-max-tokens 65536
	    :flow-gemini-model "gemini-3.1-pro"
	    :stateless-sonnet-model "claude-sonnet"
	    :queued-sonnet-model "claude-sonnet"
	    :anthropic-urgent-model "claude-opus-4-6"
	    :anthropic-ops-model "claude-sonnet-4-6"
	    :anthropic-docs-test-chore-model "claude-haiku-4-5-20251001"
	    :compress-model "gemini-3.1-pro"
	    :compress-channel "google"
	    :compress-max-tokens 2048
	    :compress-char-budget-chars 100000
	    :direct-http-timeout-secs 60
	    :router-chat-idle-timeout-secs 600
	    :gemini-pty-queue-timeout-secs 30
	    :gemini-http-queue-timeout-secs 300
	    :gemini-file-upload-timeout-secs 600
	    :gemini-file-poll-timeout-secs 300
	    :gemini-cli-absolute-timeout-secs 900
	    :gemini-cli-tool-exec-timeout-secs 300
	    :queued-sonnet-quota-throttle-secs 30
	    :queued-sonnet-default-max-tokens 1024)
	  (compression-contract
	    :checks ["node scripts/check-v3-router-policy-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.facade, `
mod chat;
mod files;
mod manage;
"mission_router_chat" => chat::handle_chat(state, args).await
"mission_router_chat_manage" => manage::handle_consolidated(state, args).await
"mission_router_chat_history" "mission_router_chat_compress" manage::handle_legacy
Unknown router_chat tool
`);

  writeFixture(root, DEFAULT_FILES.chat, `
	handle_chat apply_context_budget MAX_ROUTER_PAYLOAD_BYTES resolve_llm_credentials REQUEST_CALLER
	GeminiFileApi PreparedFile resolve_gemini_api_key is_file_denied FILE_MAX_SIZE_BINARY FILE_MAX_SIZE_TEXT
	RouterRuntimeConfig::load_for_current_dir router_config.default_chat_model router_config.chat_default_max_tokens router_config.file_chat_default_max_tokens
	router_chat_get_or_create router_chat_get_summary router_chat_load_active_history
	kb_list list_board_tasks send_with_timeout router_chat_append_messages finish_reason context_budget
	`);

  writeFixture(root, DEFAULT_FILES.files, `
FILE_DENY_PATTERNS FILE_DENY_NAMES FILE_MAX_SIZE_TEXT FILE_MAX_SIZE_BINARY resolve_gemini_api_key
is_file_denied /.ssh/ .env credentials.json
`);

  writeFixture(root, DEFAULT_FILES.manage, `
	handle_consolidated handle_legacy mission_router_chat_history mission_router_chat_list mission_router_chat_stats
	mission_router_chat_clear mission_router_chat_delete mission_router_chat_delete_message
	mission_router_chat_restore mission_router_chat_compress router_chat_load_history router_chat_list
	router_chat_stats router_chat_clear router_chat_delete router_chat_delete_message router_chat_restore
	router_chat_update_summary REQUEST_CALLER router_chat_compress RouterRuntimeConfig::load_for_current_dir
	router_config.default_chat_model router_config.compress_model router_config.compress_channel
	router_config.compress_max_tokens router_config.compress_char_budget_chars
	`);

  writeFixture(root, DEFAULT_FILES.v3Runtime, `
pub(crate) struct RouterRuntimeConfig DEFAULT_ROUTER_CHAT_MODEL DEFAULT_ROUTER_FLOW_GEMINI_MODEL
DEFAULT_ROUTER_STATELESS_SONNET_MODEL DEFAULT_ROUTER_QUEUED_SONNET_MODEL DEFAULT_ROUTER_COMPRESS_MODEL
DEFAULT_ROUTER_GEMINI_PTY_QUEUE_TIMEOUT_SECS DEFAULT_ROUTER_GEMINI_HTTP_QUEUE_TIMEOUT_SECS DEFAULT_ROUTER_QUEUED_SONNET_QUOTA_THROTTLE_SECS
DEFAULT_ROUTER_GEMINI_FILE_UPLOAD_TIMEOUT_SECS DEFAULT_ROUTER_GEMINI_FILE_POLL_TIMEOUT_SECS DEFAULT_ROUTER_GEMINI_CLI_ABSOLUTE_TIMEOUT_SECS DEFAULT_ROUTER_GEMINI_CLI_TOOL_EXEC_TIMEOUT_SECS
DEFAULT_ROUTER_ANTHROPIC_URGENT_MODEL DEFAULT_ROUTER_ANTHROPIC_OPS_MODEL DEFAULT_ROUTER_ANTHROPIC_DOCS_TEST_CHORE_MODEL
anthropic_urgent_model anthropic_ops_model anthropic_docs_test_chore_model gemini_pty_queue_timeout gemini_http_queue_timeout queued_sonnet_quota_throttle
gemini_file_upload_timeout gemini_file_poll_timeout gemini_cli_absolute_timeout gemini_cli_tool_exec_timeout
parse_router_runtime_policy find_form(source, "router-runtime-policy") direct_http_timeout :gemini-pty-queue-timeout-secs :gemini-http-queue-timeout-secs :gemini-file-upload-timeout-secs :gemini-file-poll-timeout-secs :gemini-cli-absolute-timeout-secs :gemini-cli-tool-exec-timeout-secs :queued-sonnet-quota-throttle-secs
`);

  writeFixture(root, DEFAULT_FILES.main, `
RouterRuntimeConfig::load_for_current_dir with_router_runtime_config(&router_runtime_config)
cli_cfg.resolved_model(&router_runtime_config) cli_model V3_BLUEPRINT_CONFIG_ERROR
`);

  writeFixture(root, DEFAULT_FILES.embeddingWorker, `
RouterRuntimeConfig pub model: Option<String> resolved_model(&self, router_config: &RouterRuntimeConfig)
default_model: Option<String> router_config.flow_gemini_model.clone()
`);

  writeFixture(root, DEFAULT_FILES.geminiClient, `
RouterRuntimeConfig pty_queue_timeout http_queue_timeout with_router_runtime_config
map(str::trim) filter(|model| !model.is_empty())
config.gemini_pty_queue_timeout() config.gemini_http_queue_timeout() self.pty_queue_timeout self.http_queue_timeout
`);

  writeFixture(root, DEFAULT_FILES.geminiCli, `
RouterRuntimeConfig absolute_timeout tool_exec_timeout with_router_runtime_config
config.gemini_cli_absolute_timeout() config.gemini_cli_tool_exec_timeout()
absolute timeout ({}s)
`);

  writeFixture(root, DEFAULT_FILES.geminiDriver, `
RouterRuntimeConfig RouterRuntimeConfig::load_for_project_root
router_config.flow_gemini_model.as_str() model.unwrap_or(default_model)
V3_BLUEPRINT_CONFIG_ERROR
`);

  writeFixture(root, DEFAULT_FILES.geminiFileApi, `
RouterRuntimeConfig new_with_router_runtime_config config.gemini_file_upload_timeout()
config.gemini_file_poll_timeout() poll_timeout self.poll_timeout
`);

  writeFixture(root, DEFAULT_FILES.llmGateway, `
RouterRuntimeConfig::load_for_current_dir router_config.flow_gemini_model router_config.chat_default_max_tokens
router_config.stateless_sonnet_model router_config.anthropic_urgent_model router_config.anthropic_ops_model
router_config.anthropic_docs_test_chore_model router_config.direct_http_timeout() V3_BLUEPRINT_CONFIG_ERROR
`);

  writeFixture(root, DEFAULT_FILES.sonnetGateway, `
RouterRuntimeConfig::load_for_current_dir queued_sonnet_model queued_sonnet_default_max_tokens
model: Arc<str> pub(crate) fn model(&self) -> &str queued_sonnet_quota_throttle()
quota_throttle_sleep config.direct_http_timeout() V3_BLUEPRINT_CONFIG_ERROR
`);

  writeFixture(root, DEFAULT_FILES.translationWorker, `
sonnet.model().to_string()
insert_translation(ctx.message_id, &translation, &model, duration_ms)
`);

  writeFixture(root, DEFAULT_FILES.xjpRouterClient, `
RouterRuntimeConfig::load_for_current_dir RouterRuntimeConfig::default().direct_http_timeout()
resolve_from_with_default_timeout router_config.direct_http_timeout() V3_BLUEPRINT_CONFIG_ERROR
timeout_secs unwrap_or(default_timeout)
`);

  writeFixture(root, DEFAULT_FILES.mcp, `
ToolDefinition::new "mission_router_chat" "mission_router_chat_manage"
"history" "list" "delete" "clear" "delete_message" "restore" "stats" "compress"
`);

  writeFixture(root, DEFAULT_FILES.planAdapter, `
RouterPolicyMode parse_router_policy_mode attach_router_recommendation_block compute_recommendation
DEFAULT_POLICY_PATH missiond.router-recommendation.v0 applied=false dry_run runtime_replacement
`);
  writeFixture(root, DEFAULT_FILES.predicate, 'project_context evaluate_clause arg_string glob_match');
  writeFixture(root, DEFAULT_FILES.readiness, 'load_trace_index load_backend_registry attach_trace_index_fields attach_backend_readiness_fields RICH_TRACE_THRESHOLD BackendRegistryInfo');
  writeFixture(root, DEFAULT_FILES.descriptor, 'attach_router_dispatch_descriptor dry_run_only runtime_replacement no_execution router_apply_eligible');
  writeFixture(root, DEFAULT_FILES.schemaParser, 'parse_router_policy parse_backend_registry router-policy router-backend-registry');
  writeFixture(root, DEFAULT_FILES.policyChecker, 'missiond.router-policy.v1 dry-run-only true runtime-replacement false');
  writeFixture(root, DEFAULT_FILES.registryChecker, 'missiond.router-backend-registry.v1 runtime_allowed readiness_status');
  writeFixture(root, DEFAULT_FILES.descriptorChecker, 'missiond.router-dispatch-descriptor.v1 dry_run_only runtime_replacement no_execution');

  return root;
}

function writeFixture(root, rel, source) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, source);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
