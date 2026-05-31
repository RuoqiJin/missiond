#!/usr/bin/env node
// Compatibility wrapper. The deterministic label rules now live in the daemon
// message_labeler worker and write message_label_evidence before refreshing the
// legacy message_labels projection.

import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

function usage() {
  console.log(`Usage:
  node scripts/label-claudecode-message-origin.mjs [--apply] [--json] [--limit N]

Delegates to:
  mission_conversation_query(action=label_audit)
  mission_conversation_query(action=label_backfill, source=claude_code, apply=true)

The old direct SQL labeler was intentionally removed so label rules have one
runtime owner and one evidence model.`);
}

function parseArgs(argv) {
  const opts = { apply: false, json: false, limit: 200 };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--apply') opts.apply = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--limit') opts.limit = Number(argv[++i]);
    else if (arg === '--db') i += 1; // kept for old invocations; daemon owns DB selection.
    else if (arg === '--help' || arg === '-h') {
      usage();
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  if (!Number.isInteger(opts.limit) || opts.limit < 1) {
    throw new Error('--limit must be a positive integer');
  }
  return opts;
}

function callMissionConversationQuery(args) {
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const callScript = path.join(scriptDir, 'mission-mcp-call.mjs');
  const result = spawnSync(
    process.execPath,
    [callScript, 'mission_conversation_query', JSON.stringify(args)],
    {
      encoding: 'utf8',
      maxBuffer: 1024 * 1024 * 20,
      env: {
        ...process.env,
        MISSION_MCP_CALL_TIMEOUT_MS: process.env.MISSION_MCP_CALL_TIMEOUT_MS || '120000',
      },
    },
  );
  if (result.status !== 0) {
    throw new Error(`mission_conversation_query failed (${result.status})\n${result.stderr}\n${result.stdout}`);
  }
  const response = JSON.parse(result.stdout);
  const text = response?.result?.content?.[0]?.text;
  if (typeof text === 'string') {
    try {
      return JSON.parse(text);
    } catch {
      return { text };
    }
  }
  return response;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const action = opts.apply ? 'label_backfill' : 'label_audit';
  const result = callMissionConversationQuery({
    action,
    source: 'claude_code',
    limit: opts.limit,
    apply: opts.apply,
  });
  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else {
    console.log(JSON.stringify(result, null, 2));
  }
}

main();
