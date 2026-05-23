#!/usr/bin/env node

import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const usage = `Usage:
  node scripts/agent-navigation.mjs guide --intent "change plan execution" [--project missiond] [--json]
  node scripts/agent-navigation.mjs catalog [--intent "..."] [--project missiond] [--json]
  node scripts/agent-navigation.mjs review [--json]
  node scripts/agent-navigation.mjs feedback --entry-id modify-workstation-autopilot --outcome used [--intent "..."] [--json]
  node scripts/agent-navigation.mjs suggest_entries --project jarvis [--json]
  node scripts/agent-navigation.mjs evaluate [--json]

Actions:
  guide calls mission_tool_directory(action="guide").
  catalog, review, feedback, and suggest_entries call mission_agent_navigation.
  evaluate runs the local navigation quality gate.
`;

const ACTIONS = new Set([
  'guide',
  'catalog',
  'review',
  'feedback',
  'suggest_entries',
  'suggest',
  'evaluate',
]);

function main() {
  const parsed = parseArgs(process.argv.slice(2));
  if (parsed.help) {
    console.log(usage);
    return;
  }
  const action = parsed.action === 'suggest' ? 'suggest_entries' : parsed.action;
  if (!action) die('missing action');
  if (!ACTIONS.has(action)) die(`unknown action: ${action}`);

  if (action === 'evaluate') {
    const result = evaluateQuality(parsed.json);
    writeOutput(result, parsed.json);
    process.exit(result.ok ? 0 : 1);
  }

  const args = buildToolArgs(action, parsed);
  const toolName = action === 'guide' ? 'mission_tool_directory' : 'mission_agent_navigation';
  callTool(toolName, args)
    .then((result) => {
      writeOutput(result, parsed.json);
      const ok = result?.ok ?? !result?.diagnostic;
      process.exit(ok === false ? 1 : 0);
    })
    .catch((err) => die(err.message ?? String(err)));
}

function parseArgs(argv) {
  let action = null;
  const opts = { json: false, help: false, positional: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      opts.help = true;
      continue;
    }
    if (!action && !arg.startsWith('-')) {
      action = arg;
      continue;
    }
    if (arg === '--json') {
      opts.json = true;
      continue;
    }
    if (arg.startsWith('--')) {
      const key = arg.slice(2);
      const next = argv[i + 1];
      if (next == null || next.startsWith('--')) die(`${arg} requires a value`);
      opts[camelKey(key)] = next;
      i += 1;
      continue;
    }
    opts.positional.push(arg);
  }
  opts.action = action;
  return opts;
}

function buildToolArgs(action, parsed) {
  const args = { action };
  const positionalIntent = parsed.positional.join(' ').trim();
  const intent = parsed.intent ?? parsed.query ?? (positionalIntent || undefined);
  const project = parsed.project ?? parsed.projectId ?? parsed.project_id;
  const entryId = parsed.entryId ?? parsed.entry_id;
  const agentId = parsed.agentId ?? parsed.agent_id;
  if (intent) args.intent = intent;
  if (project) args.project = project;
  if (entryId) args.entryId = entryId;
  if (parsed.surface) args.surface = parsed.surface;
  if (parsed.outcome) args.outcome = parsed.outcome;
  if (parsed.rationale) args.rationale = parsed.rationale;
  if (agentId) args.agentId = agentId;
  if (parsed.limit) args.limit = Number(parsed.limit);
  return args;
}

function evaluateQuality(jsonOutput) {
  const child = spawnSync(
    process.execPath,
    ['scripts/check-v3-agent-navigation-quality.mjs', '--json', '--check'],
    { cwd: process.cwd(), encoding: 'utf8' },
  );
  let payload;
  try {
    payload = JSON.parse(child.stdout.trim());
  } catch {
    payload = {
      ok: false,
      diagnostic: {
        code: 'AGENT_NAVIGATION_EVALUATE_FAILED',
        message: child.stderr || child.stdout || `exit ${child.status}`,
      },
    };
  }
  if (!jsonOutput && child.stderr.trim()) {
    payload.stderr = child.stderr.trim();
  }
  return payload;
}

function callTool(name, args) {
  return callMissiond('tools/call', { name, arguments: args }).then((result) => {
    const text = result?.content?.[0]?.text;
    if (!text) return result;
    try {
      return JSON.parse(text);
    } catch {
      return { ok: true, text };
    }
  });
}

function callMissiond(method, params) {
  const socketPath = resolveSocketPath();
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(socketPath, () => {
      socket.write(JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) + '\n');
    });
    let data = '';
    socket.on('data', (chunk) => {
      data += chunk.toString();
    });
    socket.on('end', () => {
      try {
        const response = JSON.parse(data.trim());
        if (response.error) reject(new Error(response.error.message || JSON.stringify(response.error)));
        else resolve(response.result);
      } catch {
        reject(new Error(`Invalid missiond response: ${data.slice(0, 200)}`));
      }
    });
    socket.on('error', reject);
    socket.setTimeout(Number(process.env.MISSIOND_AGENT_NAVIGATION_TIMEOUT_MS ?? 10000), () => {
      socket.destroy();
      reject(new Error('missiond IPC timeout'));
    });
  });
}

function resolveSocketPath() {
  if (process.env.MISSION_IPC_ENDPOINT) return process.env.MISSION_IPC_ENDPOINT;
  if (process.env.MISSION_IPC_SOCKET) return process.env.MISSION_IPC_SOCKET;
  const home = os.homedir();
  const modern = path.join(home, '.missiond', 'missiond.sock');
  const legacy = path.join(home, '.xjp-mission', 'missiond.sock');
  if (fs.existsSync(modern)) return modern;
  if (fs.existsSync(legacy)) return legacy;
  return modern;
}

function writeOutput(value, jsonOutput) {
  if (jsonOutput) {
    console.log(JSON.stringify(value, null, 2));
    return;
  }
  if (value?.schema === 'missiond.tool-directory-guide.v1') {
    printGuide(value);
  } else if (value?.schema === 'missiond.agent-navigation.catalog.v1') {
    printCatalog(value);
  } else if (value?.schema === 'missiond.agent-navigation.review.v1') {
    console.log(`review events: ${value.summary?.eventCount ?? 0}`);
  } else if (value?.schema === 'missiond.agent-navigation.feedback.v1') {
    console.log(`feedback appended: ${value.event?.id ?? 'ok'}`);
  } else {
    console.log(JSON.stringify(value, null, 2));
  }
}

function printGuide(value) {
  if (value.ok === false) {
    console.log(`${value.diagnostic?.code ?? 'ERROR'}: ${value.diagnostic?.message ?? 'guide unavailable'}`);
    return;
  }
  const entry = value.selectedEntry ?? {};
  console.log(`${entry.id ?? entry.projectId ?? 'entry'}: ${entry.label ?? value.project ?? 'navigation'}`);
  printList('readFirst', value.readFirst);
  printList('checks', value.checks);
  printList('mustNotTouch', value.mustNotTouch);
}

function printCatalog(value) {
  if (value.ok === false) {
    console.log(`${value.diagnostic?.code ?? 'ERROR'}: ${value.diagnostic?.message ?? 'catalog unavailable'}`);
    return;
  }
  const entries = value.entries ?? value.projects ?? [];
  console.log(`entries: ${entries.length}; selected: ${value.selectedEntry?.id ?? value.selectedEntry?.projectId ?? 'none'}`);
}

function printList(label, values) {
  if (!Array.isArray(values) || values.length === 0) return;
  console.log(`${label}:`);
  for (const value of values) console.log(`  - ${value}`);
}

function camelKey(key) {
  return key.replace(/-([a-z])/g, (_, ch) => ch.toUpperCase());
}

function die(message) {
  console.error(message);
  console.error(usage);
  process.exit(2);
}

main();
