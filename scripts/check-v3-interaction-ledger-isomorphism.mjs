#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const usage = `Usage:
  node scripts/check-v3-interaction-ledger-isomorphism.mjs [--json] [--repo <path>]

Checks the V3 interaction-ledger contract:
  - SSOT declares interaction-ledger / conversation-control-plane.
  - Implementation map pins the Rust/API/storage surfaces.
  - Jarvis/Interaction runtime persists user-visible lifecycle events.
  - /interactions/v1/{interaction_id}/events replays durable DB rows.
`;

let json = false;
let repoRoot = process.cwd();
const args = process.argv.slice(2);
for (let i = 0; i < args.length; i += 1) {
  const arg = args[i];
  if (arg === '--json') {
    json = true;
  } else if (arg === '--repo') {
    repoRoot = path.resolve(args[++i] ?? '');
  } else if (arg === '--help' || arg === '-h') {
    console.log(usage);
    process.exit(0);
  } else {
    console.error(`unknown arg: ${arg}`);
    console.error(usage);
    process.exit(2);
  }
}

const checks = [
  [
    'ssot interaction ledger',
    '.missiond/v3/shards/memory-knowledge-runtime.lisp',
    [
      '(interaction-ledger',
      ':schema "missiond.interaction-ledger.v1"',
      '(interaction-run-correlation',
      '(interaction-event-ledger',
      '(interaction-replay-api',
      '(conversation-control-plane',
      'conversation_events',
      'interaction.',
      'missiond.interaction-event-stream.v1',
      'GET /interactions/v1/:interaction_id/events',
      'mission_interaction status/follow',
      'conversation_messages',
      'shared_artifacts',
      'task_result_artifact',
      'Topic labels MUST be derived from the user request/topic',
      'node scripts/check-v3-interaction-ledger-isomorphism.mjs --json',
    ],
  ],
  [
    'implementation map interaction ledger',
    '.missiond/v3/shards/implementation/request-surfaces.lisp',
    [
      '(surface interaction-ledger',
      ':implements [interaction-ledger interaction-run-correlation interaction-event-ledger interaction-replay-api conversation-control-plane]',
      'crates/missiond-core/src/ws/server.rs',
      'crates/missiond-core/src/db/traits.rs',
      'crates/missiond-core/src/db/pg/conversation.rs',
      'conversation_events.event_type=interaction.*',
      'conversation_events.raw_data.interaction_id',
      'persist_interaction_event',
      'get_interaction_events',
      'insert_conversation_events_batch',
      'handle_interaction_events',
      'missiond.interaction-event-stream.v1',
    ],
  ],
  [
    'root live check',
    '.missiond/v3/missiond-blueprint.lisp',
    [
      '(live-check interaction-ledger',
      'scripts/check-v3-interaction-ledger-isomorphism.mjs',
    ],
  ],
  [
    'shard index surfaces',
    '.missiond/v3/shards/index.lisp',
    [
      'interaction-ledger',
      'conversation-control-plane',
      'implementation-request-surfaces',
    ],
  ],
  [
    'db trait replay API',
    'crates/missiond-core/src/db/traits.rs',
    [
      'async fn get_interaction_events',
      'interaction_id: &str',
      'DbResult<Vec<ConversationEvent>>',
    ],
  ],
  [
    'postgres replay query',
    'crates/missiond-core/src/db/pg/conversation.rs',
    [
      'async fn get_interaction_events',
      "event_type LIKE 'interaction.%'",
      'raw_data LIKE $1',
      'ORDER BY id ASC',
    ],
  ],
  [
    'ws persistence and replay',
    'crates/missiond-core/src/ws/server.rs',
    [
      'async fn persist_interaction_event',
      'event_type: format!("interaction.{event}")',
      'insert_conversation_events_batch',
      'async fn handle_interaction_events',
      'get_interaction_events(interaction_id, 500)',
      '"schema": "missiond.interaction-event-stream.v1"',
      '"phase": "replay_ready"',
      'strip_prefix("interaction.")',
      'MISSIOND_DB_UNAVAILABLE',
      'Self::persist_interaction_event',
      '"result_artifact"',
      '"final"',
      '"confirm_required"',
      '"board_task_created"',
      '"result_pending"',
    ],
  ],
];

const diagnostics = [];
for (const [name, rel, needles] of checks) {
  const file = path.join(repoRoot, rel);
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    diagnostics.push({ check: name, file: rel, message: 'file not found' });
    continue;
  }
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({
        check: name,
        file: rel,
        message: `missing interaction-ledger anchor: ${needle}`,
      });
    }
  }
}

const result = {
  ok: diagnostics.length === 0,
  contract: 'interaction-ledger',
  schema: 'missiond.interaction-ledger.v1',
  checks: checks.length,
  diagnostics,
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else if (result.ok) {
  console.log('v3 interaction-ledger Lisp/code isomorphism check OK');
} else {
  for (const diagnostic of diagnostics) {
    console.error(`${diagnostic.file}: ${diagnostic.message}`);
  }
  console.error(`v3 interaction-ledger check FAILED -- ${diagnostics.length} diagnostic(s)`);
}

process.exit(result.ok ? 0 : 1);
