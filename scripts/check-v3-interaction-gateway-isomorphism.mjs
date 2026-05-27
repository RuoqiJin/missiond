#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const repo = process.cwd();
const json = process.argv.includes('--json');

function read(rel) {
  return fs.readFileSync(path.join(repo, rel), 'utf8');
}

function exists(rel) {
  return fs.existsSync(path.join(repo, rel));
}

const files = {
  v3: '.missiond/v3/shards/pillar-flow-map.lisp',
  requestSurfaces: '.missiond/v3/shards/implementation/request-surfaces.lisp',
  v2: '.missiond/v3/shards/v2-convergence-map.lisp',
  workflow: '.missiond/workflows/intent-intake-grounding.lisp',
  frontend: '.missiond/frontend/board-blueprint.lisp',
  server: 'crates/missiond-core/src/ws/server.rs',
  jarvisChainSmoke: 'scripts/smoke-jarvis-chain.mjs',
  jarvisInteractionSmoke: 'scripts/smoke-jarvis-interaction.mjs',
  jarvisIntentPlanDispatchSmoke: 'scripts/smoke-jarvis-intent-plan-dispatch.mjs',
  deployDaemon: 'scripts/deploy-daemon.sh',
  boardTypes: 'crates/missiond-core/src/types/board.rs',
  boardStore: 'crates/missiond-core/src/db/pg/board.rs',
  boardMetadataMigration: 'crates/missiond-core/migrations/20260525000000_board_task_runtime_metadata.sql',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  boardFrontendTypes: 'packages/board/src/types.ts',
  mcpTool: 'crates/missiond-mcp/src/tools/comm/interaction.rs',
  mcpMod: 'crates/missiond-mcp/src/tools/mod.rs',
  daemonHandler: 'crates/missiond-daemon/src/handlers/comm/interaction.rs',
  daemonMod: 'crates/missiond-daemon/src/handlers/mod.rs',
  gateway: 'crates/missiond-mcp/src/gen_gateway.rs',
};

const diagnostics = [];

function requireFile(key) {
  const rel = files[key];
  if (!exists(rel)) {
    diagnostics.push({ file: rel, message: 'required interaction-gateway file is missing' });
    return '';
  }
  return read(rel);
}

function requireIncludes(key, needles) {
  const rel = files[key];
  const text = requireFile(key);
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({ file: rel, message: `missing interaction-gateway anchor: ${needle}` });
    }
  }
  return text;
}

requireIncludes('v3', [
  '(function interaction-gateway',
  ':surface interaction-gateway',
  '/interactions/v1/messages',
  'InteractionEnvelope',
  'handle_chat_completions_interaction_adapter',
  'openai_request_to_interaction_envelope',
  'PermissionContext',
  '/oidc/userinfo',
  'MISSIOND_INTERACTION_AUTH_USERINFO_URL',
  'INTERACTION_AUTH_UNAVAILABLE',
  'intent_draft',
  'plan_draft',
  'task-result-artifact',
]);

requireIncludes('requestSurfaces', [
  '(surface interaction-gateway',
  'crates/missiond-core/src/ws/server.rs',
  'crates/missiond-mcp/src/tools/comm/interaction.rs',
  'crates/missiond-daemon/src/handlers/comm/interaction.rs',
  'mission_interaction',
  'MISSIOND_INTERACTION_AUTH_USERINFO_URL',
  'INTERACTION_AUTH_UNAVAILABLE',
  'missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN',
]);

requireIncludes('v2', [
  '(tool-group interaction-gateway-tools',
  ':v3-function interaction-gateway',
  ':tools [mission_interaction]',
]);

requireIncludes('workflow', [
  'interaction-gateway',
  'InteractionEnvelope',
  'PermissionContext',
  'Human/external broad requests must emit confirm_required',
]);

requireIncludes('frontend', [
  'interaction_received',
  'interaction_permission_resolved',
  'interaction_intent_draft',
  'interaction_plan_draft',
  'interaction_board_task_created',
  'interaction_result_artifact',
]);

requireIncludes('server', [
  'struct InteractionEnvelope',
  'handle_interaction_messages',
  'handle_interaction_events',
  'handle_chat_completions_interaction_adapter',
  'openai_request_to_interaction_envelope',
  'normalize_public_jarvis_path',
  '/jarvis/api/monitor/jarvis',
  '/internal/jarvis/slot/ensure',
  'missiond.jarvis-slot-ensure.v1',
  'POST /interactions/v1/messages',
  'GET /interactions/v1/',
  'missiond.interaction-envelope.v1',
  'MISSIOND_JARVIS_SLOT_AUTO_HEAL',
  'MISSIOND_JARVIS_SLOT_AUTO_HEAL_TIMEOUT_SECS',
  'resolve_interaction_auth',
  'MISSIOND_INTERACTION_AUTH_USERINFO_URL',
  'INTERACTION_AUTH_UNAVAILABLE',
  'auth-userinfo',
  'permission_resolved',
  'intent_draft',
  'plan_draft',
  'board_task_created',
  'dispatch_accepted',
  'result_pending',
  'terminal_task_result',
  'runtime_metadata: Some(meta)',
  'See runtime_metadata for grounding, intent, plan',
  'jarvis_slot_auto_heal_enabled',
  'ensure_jarvis_slot_ready_for_chat',
  'handle_jarvis_slot_ensure',
  'maybe_auto_heal_jarvis_slot',
  'JARVIS_SLOT_MANAGER_UNAVAILABLE',
  'JARVIS_SLOT_UNAVAILABLE',
]);

requireIncludes('jarvisChainSmoke', [
  'missiond.jarvis-chain-smoke.v1',
  '/api/monitor/jarvis',
  'missiond.jarvis-chain-monitor.v1',
  'public-entry',
  'default-slot-readiness',
]);

requireIncludes('jarvisInteractionSmoke', [
  'missiond.jarvis-interaction-smoke.v1',
  'MISSIOND_JARVIS_SMOKE_TOKEN',
  'MISSIOND_INTERACTION_SERVICE_TOKEN',
  'MISSIOND_JARVIS_SMOKE_SECRET_REF',
  'missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN',
  'xjp',
  'secret',
  '--raw',
  'intent_draft',
  'confirm_required',
  'JARVIS_CONFIRMATION_BYPASS',
]);

requireIncludes('jarvisIntentPlanDispatchSmoke', [
  'missiond.jarvis-intent-plan-dispatch-smoke.v1',
  'intent_draft',
  'plan_draft',
  'board_task_created',
  'dispatch_accepted',
  'result_pending',
  'JARVIS_DISPATCH_FOLLOW',
  'follow_payload',
  'FOLLOW_TERMINAL_RESULT_MISSING',
  'FOLLOW_FINAL_WITHOUT_RESULT_ARTIFACT',
  'NON_TERMINAL_FINAL',
  'terminal_task_result',
]);

requireIncludes('deployDaemon', [
  'MISSIOND_INTERACTION_SERVICE_TOKEN',
  'MISSIOND_INTERACTION_AUTH_USERINFO_URL',
  'MISSIOND_INTERACTION_AUTH_TIMEOUT_MS',
  'plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_INTERACTION_SERVICE_TOKEN"',
]);

const serverText = requireFile('server');
if (!/POST \/v1\/chat\/completions[\s\S]{0,600}handle_chat_completions_interaction_adapter/.test(serverText)) {
  diagnostics.push({
    file: files.server,
    message: 'legacy /v1/chat/completions route must enter interaction-gateway adapter, not direct PTY dispatch',
  });
}
if (
  !serverText.includes('normalize_public_jarvis_path(path)') ||
  !serverText.includes('normalized_path == "/v1/chat/completions"') ||
  !serverText.includes('normalized_path == "/api/monitor/jarvis"') ||
  !serverText.includes('path == "/internal/jarvis/slot/ensure"')
) {
  diagnostics.push({
    file: files.server,
    message: 'public /jarvis/* routes must normalize before HTTP demux falls back to WebSocket handling',
  });
}

requireIncludes('boardTypes', [
  'runtime_metadata: serde_json::Value',
  'runtime_metadata: Option<serde_json::Value>',
  'rename = "runtimeMetadata"',
  'skip_serializing_if = "is_empty_object"',
]);

requireIncludes('boardStore', [
  'runtime_metadata',
  'INSERT INTO board_tasks',
  '.bind(&task.runtime_metadata)',
]);

const boardStoreText = requireFile('boardStore');
if (
  !/runtime_metadata:\s*self\s*\.\s*runtime_metadata\s*\.\s*unwrap_or_else/s.test(boardStoreText)
) {
  diagnostics.push({
    file: files.boardStore,
    message: 'Board row mapping must hydrate runtime_metadata from JSONB with an empty-object fallback',
  });
}

requireIncludes('boardMetadataMigration', [
  'ADD COLUMN IF NOT EXISTS runtime_metadata JSONB',
  'idx_board_tasks_runtime_metadata_gin',
  "runtime_metadata->>'interaction_id'",
  "runtime_metadata->>'grounding_context_id'",
]);

const autopilotText = requireIncludes('autopilot', [
  'fn extract_board_task_dispatch_metadata_field',
  'json_metadata_value_to_string',
  'extract_board_task_dispatch_metadata_field(task, "engine_hint")',
  'extract_board_task_dispatch_metadata_field(task, "pool_hint")',
  'task.runtime_metadata',
]);
if (autopilotText.includes('extract_dispatch_metadata_field(&task.description, field)')) {
  diagnostics.push({
    file: files.autopilot,
    message: 'runtime control must not parse BoardTask.description for dispatch metadata',
  });
}

requireIncludes('boardFrontendTypes', [
  'runtimeMetadata?: Record<string, unknown>;',
]);

requireIncludes('mcpTool', [
  'mission_interaction',
  'confirm_intent',
  'confirm_plan',
  'InteractionEnvelope',
]);

requireIncludes('mcpMod', [
  'interaction',
  'tools.extend(interaction::definitions())',
]);

requireIncludes('daemonHandler', [
  'missiond.interaction-envelope.v1',
  'missiond.interaction-confirmation.v1',
  'missiond.interaction-status.v1',
  'receive|confirm_intent|confirm_plan|follow|status',
]);

requireIncludes('daemonMod', [
  'interaction',
  '"mission_interaction"',
]);

requireIncludes('gateway', [
  '"mission_question" | "mission_interaction"',
]);

const ok = diagnostics.length === 0;
if (json) {
  console.log(JSON.stringify({
    ok,
    schema: 'missiond.v3.interaction-gateway-isomorphism.v1',
    diagnostics,
    checked_files: Object.values(files),
  }, null, 2));
} else if (ok) {
  console.log('v3 interaction-gateway isomorphism check OK');
} else {
  for (const diagnostic of diagnostics) {
    console.error(`${diagnostic.file}: ${diagnostic.message}`);
  }
  console.error(`v3 interaction-gateway isomorphism check FAILED — ${diagnostics.length} diagnostic(s)`);
}

process.exit(ok ? 0 : 1);
