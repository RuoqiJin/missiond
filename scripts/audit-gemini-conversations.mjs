#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const usage = `Usage:
  node scripts/audit-gemini-conversations.mjs [--json]

Audits Gemini CLI durable chat files against MissionD conversation storage.
Reports raw sessions missing in DB, DB sessions whose raw file disappeared,
and Gemini tool_call rows still pending after tool_result messages exist.
`;

function walk(dir, out = []) {
  let entries = [];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const entry of entries) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(file, out);
    } else if (/\/chats\/session-.*\.jsonl?$/.test(file)) {
      out.push(file);
    }
  }
  return out;
}

function parseSession(file) {
  const text = fs.readFileSync(file, 'utf8');
  try {
    return JSON.parse(text);
  } catch {
    const session = { messages: [] };
    for (const line of text.split(/\n/)) {
      if (!line.trim()) continue;
      const value = JSON.parse(line);
      if (value.sessionId) {
        session.sessionId = value.sessionId;
        session.projectHash = value.projectHash;
        session.startTime = value.startTime;
        session.lastUpdated = value.lastUpdated;
      } else if (value.id && value.type) {
        session.messages.push(value);
      }
    }
    return session;
  }
}

function psql(query) {
  return execFileSync('psql', ['-d', 'missiond', '-A', '-F', '\t', '-t', '-c', query], {
    encoding: 'utf8',
  }).trim();
}

function rows(query) {
  const output = psql(query);
  if (!output) return [];
  return output.split('\n').map((line) => line.split('\t'));
}

function main() {
  let json = false;
  for (const arg of process.argv.slice(2)) {
    if (arg === '--json') json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  const rawFiles = walk(path.join(os.homedir(), '.gemini', 'tmp'));
  const raw = rawFiles.map((file) => {
    const session = parseSession(file);
    const messages = session.messages || [];
    return {
      file,
      sessionId: session.sessionId,
      messages: messages.length,
      toolCalls: messages.reduce((sum, msg) => sum + (msg.toolCalls?.length || 0), 0),
      earliest: messages[0]?.timestamp || session.startTime || null,
      latest: messages.at(-1)?.timestamp || session.lastUpdated || null,
    };
  }).filter((s) => s.sessionId);

  const conversations = rows(`
    SELECT id, COALESCE(jsonl_path, ''), message_count
      FROM conversations
     WHERE source = 'gemini_cli'
  `).map(([id, jsonlPath, messageCount]) => ({
    id,
    jsonlPath,
    messageCount: Number(messageCount || 0),
  }));
  const dbIds = new Set(conversations.map((c) => c.id));
  const rawPaths = new Set(raw.map((r) => r.file));

  const missingInDb = raw.filter((r) => !dbIds.has(r.sessionId));
  const rawMissing = conversations.filter((c) => c.jsonlPath && !rawPaths.has(c.jsonlPath));
  const [messageTotal, toolUseMessages, toolResultMessages] = rows(`
    SELECT COUNT(*)::text,
           COUNT(*) FILTER (WHERE has_tool_use)::text,
           COUNT(*) FILTER (WHERE has_tool_result)::text
      FROM conversation_messages m
      JOIN conversations c ON c.id = m.session_id
     WHERE c.source = 'gemini_cli'
  `)[0]?.map(Number) || [0, 0, 0];
  const [toolCallTotal, pendingToolCalls, completedToolCalls] = rows(`
    SELECT COUNT(*)::text,
           COUNT(*) FILTER (WHERE tc.status = 'pending')::text,
           COUNT(*) FILTER (WHERE tc.status IN ('success','error'))::text
      FROM conversation_tool_calls tc
      JOIN conversations c ON c.id = tc.session_id
     WHERE c.source = 'gemini_cli'
  `)[0]?.map(Number) || [0, 0, 0];

  const result = {
    ok: missingInDb.length === 0 && pendingToolCalls === 0,
    rawSessions: raw.length,
    dbConversations: conversations.length,
    rawMessages: raw.reduce((sum, r) => sum + r.messages, 0),
    rawToolCalls: raw.reduce((sum, r) => sum + r.toolCalls, 0),
    dbMessages: messageTotal,
    dbToolUseMessages: toolUseMessages,
    dbToolResultMessages: toolResultMessages,
    dbToolCalls: toolCallTotal,
    pendingToolCalls,
    completedToolCalls,
    missingInDb: missingInDb.length,
    rawMissing: rawMissing.length,
    missingSamples: missingInDb.slice(0, 20),
    rawMissingSamples: rawMissing.slice(0, 20),
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else {
    console.log(`Gemini raw sessions: ${result.rawSessions}`);
    console.log(`MissionD gemini_cli conversations: ${result.dbConversations}`);
    console.log(`Raw sessions missing in DB: ${result.missingInDb}`);
    console.log(`DB conversations missing raw file: ${result.rawMissing}`);
    console.log(`Raw tool calls: ${result.rawToolCalls}`);
    console.log(`DB tool calls: ${result.dbToolCalls}`);
    console.log(`Pending DB tool calls: ${result.pendingToolCalls}`);
  }

  process.exit(result.ok ? 0 : 1);
}

main();
