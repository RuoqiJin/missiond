#!/usr/bin/env node

import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
const json = args.includes('--json');
const sessionLimit = numberArg('--session-limit', 20);
const tail = numberArg('--tail', 1000);
const timeoutMs = numberArg('--timeout-ms', 30_000);

const sessions = await callTool('mission_conversation_query', {
  action: 'list',
  conversationType: 'all',
  limit: sessionLimit * 3,
}, timeoutMs);

const codexSessions = (Array.isArray(sessions) ? sessions : [])
  .filter((s) => String(s.source || '').includes('codex'))
  .slice(0, sessionLimit);

const reports = [];
for (const session of codexSessions) {
  const detail = await callTool('mission_conversation_query', {
    action: 'get',
    sessionId: session.id,
    tail,
  }, timeoutMs);
  const messages = Array.isArray(detail?.messages) ? detail.messages : [];
  const groups = new Map();
  for (const message of messages) {
    const key = duplicateKey(message);
    const group = groups.get(key) || {
      role: message.role,
      timestamp: message.timestamp,
      contentPreview: preview(message.content),
      ids: [],
    };
    group.ids.push(message.id);
    groups.set(key, group);
  }
  const duplicates = [...groups.values()]
    .filter((g) => g.ids.length > 1)
    .map((g) => ({
      ...g,
      keepId: g.ids[0],
      duplicateIds: g.ids.slice(1),
      duplicateCount: g.ids.length - 1,
    }));
  if (duplicates.length > 0) {
    reports.push({
      sessionId: session.id,
      title: session.title,
      source: session.source,
      duplicateGroups: duplicates.length,
      duplicateRows: duplicates.reduce((sum, g) => sum + g.duplicateCount, 0),
      groups: duplicates,
    });
  }
}

const result = {
  ok: true,
  mode: 'dry-run',
  scannedSessions: codexSessions.length,
  sessionsWithDuplicates: reports.length,
  duplicateRows: reports.reduce((sum, r) => sum + r.duplicateRows, 0),
  reports,
  nextAction: reports.length > 0
    ? 'Deploy deterministic Codex message_uuid, then run a reviewed DB cleanup that keeps keepId and removes duplicateIds.'
    : 'No duplicate Codex conversation rows detected in scanned sessions.',
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else {
  console.log(`Codex conversation duplicate dry-run: ${result.duplicateRows} duplicate row(s) in ${reports.length} session(s).`);
  for (const report of reports) {
    console.log(`- ${report.sessionId}: ${report.duplicateRows} duplicate row(s), ${report.duplicateGroups} group(s)`);
  }
  console.log(result.nextAction);
}

function duplicateKey(message) {
  const uuid = typeof message.messageUuid === 'string' && message.messageUuid.trim()
    ? message.messageUuid
    : typeof message.message_uuid === 'string' && message.message_uuid.trim()
      ? message.message_uuid
      : null;
  if (uuid) return `uuid:${uuid}`;
  return [
    'fallback',
    message.role || '',
    message.timestamp || '',
    normalize(message.content || ''),
    JSON.stringify(message.metadata || null),
  ].join('\u001f');
}

function normalize(value) {
  return String(value).replace(/\s+/g, ' ').trim();
}

function preview(value) {
  const text = normalize(value);
  return text.length > 120 ? `${text.slice(0, 119)}…` : text;
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
