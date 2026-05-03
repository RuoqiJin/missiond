#!/usr/bin/env node

import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
const json = args.includes('--json');
const sessionLimit = numberArg('--session-limit', 50);
const tail = numberArg('--tail', 400);
const timeoutMs = numberArg('--timeout-ms', 30_000);

const sessionsResult = await callTool('mission_conversation_query', {
  action: 'list',
  conversationType: 'all',
  limit: sessionLimit * 4,
}, timeoutMs);

const claudeSessions = (Array.isArray(sessionsResult) ? sessionsResult : [])
  .filter((s) => String(s.source || '').includes('claude'))
  .slice(0, sessionLimit);

const reports = [];
for (const session of claudeSessions) {
  const detail = await callTool('mission_conversation_query', {
    action: 'get',
    sessionId: session.id,
    tail,
    includeRaw: true,
  }, timeoutMs);
  const messages = Array.isArray(detail?.messages) ? detail.messages : [];
  const suspects = {
    systemWorkerPromptSuspects: [],
    userInWorkerSessionSuspects: [],
    missingRawRoleRows: [],
  };
  for (const message of messages) {
    const text = normalize(message.content || '');
    const roleDisplay = String(message.roleDisplay || '');
    const isWorkerSession = Boolean(session.slotId) || roleDisplay.startsWith('slot-');
    if (message.role === 'system' && looksLikeWorkerPrompt(text)) {
      suspects.systemWorkerPromptSuspects.push(sample(message, session));
    }
    if (message.role === 'user' && isWorkerSession && !isInteractiveConversation(session)) {
      suspects.userInWorkerSessionSuspects.push(sample(message, session));
    }
    if (
      String(session.source || '') === 'claude_code'
      && ['worker_user', 'agent_user', 'agent_assistant', 'tool_result', 'thinking'].includes(message.role)
      && !message.rawRole
    ) {
      suspects.missingRawRoleRows.push(sample(message, session));
    }
  }
  const counts = Object.fromEntries(
    Object.entries(suspects).map(([key, rows]) => [key, rows.length]),
  );
  const total = Object.values(counts).reduce((sum, count) => sum + count, 0);
  if (total > 0) {
    reports.push({
      sessionId: session.id,
      title: session.title,
      source: session.source,
      slotId: session.slotId,
      conversationType: session.conversationType,
      counts,
      samples: Object.fromEntries(
        Object.entries(suspects).map(([key, rows]) => [key, rows.slice(0, 5)]),
      ),
    });
  }
}

const totals = reports.reduce(
  (acc, report) => {
    for (const [key, value] of Object.entries(report.counts)) {
      acc[key] = (acc[key] || 0) + value;
    }
    return acc;
  },
  {},
);

const result = {
  ok: true,
  mode: 'dry-run',
  scannedSessions: claudeSessions.length,
  sessionsWithSuspects: reports.length,
  totals,
  reports,
  nextAction: reports.length > 0
    ? 'Review samples, deploy worker_user/rawRole normalization, then design a separate reviewed DB backfill if historical rows need correction.'
    : 'No obvious ClaudeCode role-attribution suspects detected in scanned sessions.',
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else {
  console.log(`ClaudeCode role-attribution dry-run: ${reports.length} suspect session(s).`);
  for (const [key, value] of Object.entries(totals)) {
    console.log(`- ${key}: ${value}`);
  }
  console.log(result.nextAction);
}

function looksLikeWorkerPrompt(text) {
  return /Execute MissionD task|BoardTask ID|Task contract SSOT|Read and follow the thin brief|When done, write the task report|completion protocol|context-pack path|write_scope|must_not_touch/i.test(text);
}

function isInteractiveConversation(session) {
  return ['jarvis', 'user'].includes(String(session.conversationType || ''));
}

function sample(message, session) {
  return {
    id: message.id,
    role: message.role,
    rawRole: message.rawRole ?? null,
    roleDisplay: message.roleDisplay ?? null,
    timestamp: message.timestamp,
    conversationType: session.conversationType,
    slotId: session.slotId ?? null,
    contentPreview: preview(message.content),
  };
}

function normalize(value) {
  return String(value).replace(/\s+/g, ' ').trim();
}

function preview(value) {
  const text = normalize(value);
  return text.length > 160 ? `${text.slice(0, 159)}…` : text;
}

function numberArg(name, fallback) {
  const idx = args.indexOf(name);
  if (idx === -1 || idx + 1 >= args.length) return fallback;
  const value = Number(args[idx + 1]);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

async function callTool(name, toolArgs = {}, callTimeoutMs = timeoutMs) {
  const result = await callMissiond('tools/call', { name, arguments: toolArgs }, callTimeoutMs);
  const text = result?.content?.[0]?.text;
  return text ? JSON.parse(text) : result;
}

function callMissiond(method, params, callTimeoutMs = timeoutMs) {
  return new Promise((resolve, reject) => {
    const socketPath = resolveSocketPath();
    const socket = net.createConnection(socketPath, () => {
      socket.write(JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) + '\n');
    });
    let data = '';
    socket.on('data', (chunk) => {
      data += chunk.toString();
    });
    socket.on('end', () => {
      try {
        const resp = JSON.parse(data.trim());
        if (resp.error) reject(new Error(resp.error.message || JSON.stringify(resp.error)));
        else resolve(resp.result);
      } catch {
        reject(new Error(`Invalid missiond response: ${data.slice(0, 200)}`));
      }
    });
    socket.on('error', reject);
    socket.setTimeout(callTimeoutMs, () => {
      socket.destroy();
      reject(new Error(`missiond IPC timeout after ${callTimeoutMs}ms`));
    });
  });
}

function resolveSocketPath() {
  if (process.env.MISSION_IPC_ENDPOINT) return process.env.MISSION_IPC_ENDPOINT;
  if (process.env.MISSION_IPC_SOCKET) return process.env.MISSION_IPC_SOCKET;
  const home = os.homedir();
  const current = path.join(home, '.missiond', 'missiond.sock');
  const legacy = path.join(home, '.xjp-mission', 'missiond.sock');
  if (fs.existsSync(current)) return current;
  if (fs.existsSync(legacy)) return legacy;
  return current;
}
