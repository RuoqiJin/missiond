#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-conversation-session-management.mjs [--json] [--static-only]

Checks the V3 conversation-session-management Lisp/code isomorphism contract:
  - Migration adds user_id, tenant_id, application_id, channel, topic_id, topic_label, context_capsule_hash.
  - Rust Conversation struct carries all session management fields.
  - PG row_to_conversation extracts all session management fields.
  - upsert_conversation binds all session management fields.
  - ConversationStore trait declares resolve_active_session and bind_context_capsule.
  - SSOT shard declares conversation-session-management policy.
`;

const DEFAULT_FILES = {
  shard: '.missiond/v3/shards/memory-knowledge-runtime.lisp',
  migration: 'crates/missiond-core/migrations/20260526000000_conversation_session_management.sql',
  types: 'crates/missiond-core/src/types/conversation.rs',
  traits: 'crates/missiond-core/src/db/traits.rs',
  pgConversation: 'crates/missiond-core/src/db/pg/conversation.rs',
  pgMessage: 'crates/missiond-core/src/db/pg/message.rs',
  pgObservability: 'crates/missiond-core/src/db/pg/observability.rs',
  wsServer: 'crates/missiond-core/src/ws/server.rs',
  conversationQueryHandler: 'crates/missiond-daemon/src/handlers/comm/conversation/query.rs',
  contextCapsule: 'crates/missiond-daemon/src/handlers/knowledge/context_capsule.rs',
  contextGather: 'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
};

const SESSION_COLUMNS = [
  'user_id',
  'tenant_id',
  'application_id',
  'channel',
  'topic_id',
  'topic_label',
  'context_capsule_hash',
];

function main() {
  const args = process.argv.slice(2);
  let json = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--static-only') {
      // static-only is the default; accept for compatibility
    }
  }

  const repoRoot = process.cwd();
  const diagnostics = checkFiles(repoRoot);
  const result = {
    ok: diagnostics.length === 0,
    contract: 'conversation-session-management',
    schema: 'missiond.conversation-session-management.v1',
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 conversation-session-management Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 conversation-session-management check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(DEFAULT_FILES)) {
    const full = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(full, 'utf8');
    } catch {
      diagnostics.push({ file: rel, message: 'file not found' });
    }
  }

  if (sources.shard) {
    if (!sources.shard.includes('conversation-session-management')) {
      diagnostics.push({
        file: DEFAULT_FILES.shard,
        message: 'SSOT shard missing conversation-session-management policy declaration',
      });
    }
    for (const col of SESSION_COLUMNS) {
      if (!sources.shard.includes(col)) {
        diagnostics.push({
          file: DEFAULT_FILES.shard,
          message: `SSOT shard missing column declaration: ${col}`,
        });
      }
    }
  }

  if (sources.migration) {
    for (const col of SESSION_COLUMNS) {
      if (!sources.migration.includes(col)) {
        diagnostics.push({
          file: DEFAULT_FILES.migration,
          message: `Migration missing column: ${col}`,
        });
      }
    }
    if (!sources.migration.includes('idx_conv_session_resolve')) {
      diagnostics.push({
        file: DEFAULT_FILES.migration,
        message: 'Migration missing composite session resolution index',
      });
    }
  }

  if (sources.types) {
    for (const col of SESSION_COLUMNS) {
      if (!sources.types.includes(`pub ${col}`)) {
        diagnostics.push({
          file: DEFAULT_FILES.types,
          message: `Conversation struct missing field: ${col}`,
        });
      }
    }
  }

  if (sources.traits) {
    if (!sources.traits.includes('resolve_active_session')) {
      diagnostics.push({
        file: DEFAULT_FILES.traits,
        message: 'ConversationStore trait missing resolve_active_session method',
      });
    }
    if (!sources.traits.includes('bind_context_capsule')) {
      diagnostics.push({
        file: DEFAULT_FILES.traits,
        message: 'ConversationStore trait missing bind_context_capsule method',
      });
    }
    if (!sources.traits.includes('jarvis_get_or_create_scoped')) {
      diagnostics.push({
        file: DEFAULT_FILES.traits,
        message: 'ObservabilityStore trait missing scoped Jarvis session resolver',
      });
    }
  }

  if (sources.pgConversation) {
    for (const col of SESSION_COLUMNS) {
      if (!sources.pgConversation.includes(`"${col}"`)) {
        diagnostics.push({
          file: DEFAULT_FILES.pgConversation,
          message: `PG row_to_conversation missing field extraction: ${col}`,
        });
      }
    }
    for (const col of ['user_id', 'tenant_id', 'application_id', 'channel']) {
      if (sources.pgConversation.includes(`AND ${col} IS NULL`)) {
        diagnostics.push({
          file: DEFAULT_FILES.pgConversation,
          message: `Session resolver must use additive filters; omitted ${col} must not force IS NULL`,
        });
      }
    }
  }

  if (sources.pgObservability) {
    for (const needle of [
      'jarvis_get_or_create_scoped',
      'resolve_active_session',
      'user_id = COALESCE(user_id',
      'topic_id = COALESCE(topic_id',
    ]) {
      if (!sources.pgObservability.includes(needle)) {
        diagnostics.push({
          file: DEFAULT_FILES.pgObservability,
          message: `Scoped Jarvis session implementation missing: ${needle}`,
        });
      }
    }
  }

  if (sources.wsServer) {
    for (const needle of [
      'conversation_scope_from_permission',
      'conversation_scope_from_request',
      'jarvis_get_or_create_scoped',
      'context_capsule_hash',
      'bind_context_capsule',
      'topic_id',
    ]) {
      if (!sources.wsServer.includes(needle)) {
        diagnostics.push({
          file: DEFAULT_FILES.wsServer,
          message: `Jarvis/OpenAI gateway missing end-to-end session/capsule hook: ${needle}`,
        });
      }
    }
  }

  if (sources.conversationQueryHandler) {
    for (const needle of ['user_id', 'tenant_id', 'application_id', 'channel']) {
      if (!sources.conversationQueryHandler.includes(needle)) {
        diagnostics.push({
          file: DEFAULT_FILES.conversationQueryHandler,
          message: `Conversation query handler does not accept isolation filter: ${needle}`,
        });
      }
    }
  }

  if (sources.pgMessage) {
    for (const needle of [
      'push_scope_conditions',
      '"user_id"',
      '"tenant_id"',
      '"application_id"',
      '"channel"',
      'semantic_conversation_search',
    ]) {
      if (!sources.pgMessage.includes(needle)) {
        diagnostics.push({
          file: DEFAULT_FILES.pgMessage,
          message: `Conversation search store does not enforce isolation scope: ${needle}`,
        });
      }
    }
  }

  if (sources.contextCapsule) {
    if (!sources.contextCapsule.includes('generate_lisp_capsule')) {
      diagnostics.push({
        file: DEFAULT_FILES.contextCapsule,
        message: 'Context capsule module missing generate_lisp_capsule function',
      });
    }
    if (!sources.contextCapsule.includes('CapsuleIsolation')) {
      diagnostics.push({
        file: DEFAULT_FILES.contextCapsule,
        message: 'Context capsule module missing CapsuleIsolation struct',
      });
    }
  }

  if (sources.contextGather) {
    for (const needle of [
      'context_capsule_hash',
      'bind_context_capsule',
      'set_conversation_topic_vectors',
      'permission_context',
      'conversation_id',
      'isolation_scope',
    ]) {
      if (!sources.contextGather.includes(needle)) {
        diagnostics.push({
          file: DEFAULT_FILES.contextGather,
          message: `Context gather handler missing capsule/session binding hook: ${needle}`,
        });
      }
    }
  }

  return diagnostics;
}

main();
