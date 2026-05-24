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
  'POST /interactions/v1/messages',
  'GET /interactions/v1/',
  'missiond.interaction-envelope.v1',
  'resolve_interaction_auth',
  'MISSIOND_INTERACTION_AUTH_USERINFO_URL',
  'INTERACTION_AUTH_UNAVAILABLE',
  'auth-userinfo',
  'permission_resolved',
  'intent_draft',
  'plan_draft',
  'board_task_created',
  'result_pending',
]);

const serverText = requireFile('server');
if (!/POST \/v1\/chat\/completions[\s\S]{0,600}handle_chat_completions_interaction_adapter/.test(serverText)) {
  diagnostics.push({
    file: files.server,
    message: 'legacy /v1/chat/completions route must enter interaction-gateway adapter, not direct PTY dispatch',
  });
}

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
