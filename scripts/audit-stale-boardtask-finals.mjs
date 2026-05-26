#!/usr/bin/env node

import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
const json = args.includes('--json');
const limit = numberArg('--limit', 100);
const taskId = stringArg('--task-id');
const timeoutMs = numberArg('--timeout-ms', 30_000);

const diagnostics = [];
const reports = [];

try {
  const taskRefs = taskId
    ? [{ id: taskId }]
    : await recentTasks(limit);

  for (const taskRef of taskRefs.slice(0, limit)) {
    const detail = await callTool('mission_board_query', {
      action: 'get',
      id: taskRef.id,
      includeChildren: false,
    });
    const task = detail?.task || detail;
    if (!task?.id) continue;
    const taskReport = await auditTask(task, detail?.notes || []);
    if (taskReport.findings.length > 0) reports.push(taskReport);
  }

  finish({
    ok: reports.length === 0,
    schema: 'missiond.stale-boardtask-final-audit.v1',
    mode: 'dry-run',
    scanned_tasks: taskRefs.length,
    reports,
    diagnostics,
  }, reports.length === 0 ? 0 : 1);
} catch (error) {
  finish({
    ok: false,
    schema: 'missiond.stale-boardtask-final-audit.v1',
    mode: 'dry-run',
    scanned_tasks: 0,
    reports,
    diagnostics: [{ code: 'STALE_FINAL_AUDIT_FAILED', message: error.message }],
  }, 2);
}

async function recentTasks(max) {
  const statuses = ['running', 'done', 'blocked', 'failed'];
  const byId = new Map();
  for (const status of statuses) {
    const rows = await callTool('mission_board_query', {
      action: 'list',
      status,
      includeHidden: true,
    });
    for (const row of Array.isArray(rows) ? rows : []) {
      if (row?.id && !byId.has(row.id)) byId.set(row.id, row);
    }
  }
  return [...byId.values()]
    .sort((a, b) => String(b.updatedAt || b.updated_at || '').localeCompare(String(a.updatedAt || a.updated_at || '')))
    .slice(0, max);
}

async function auditTask(task, notes) {
  const claimedAt = dateMs(task.claimedAt || task.claimed_at);
  const conversations = await callTool('mission_conversation_query', {
    action: 'list',
    conversationType: 'all',
    taskId: task.id,
    limit: 50,
  }).catch((error) => {
    diagnostics.push({
      code: 'CONVERSATION_QUERY_FAILED',
      task_id: task.id,
      message: error.message,
    });
    return [];
  });

  const findings = [];
  for (const conversation of Array.isArray(conversations) ? conversations : []) {
    const endedAt = dateMs(conversation.endedAt || conversation.ended_at);
    if (claimedAt && endedAt && endedAt < claimedAt) {
      findings.push({
        code: 'CONVERSATION_ENDED_BEFORE_CLAIM',
        conversation_id: conversation.id,
        task_claimed_at: task.claimedAt || task.claimed_at,
        conversation_ended_at: conversation.endedAt || conversation.ended_at,
        recommendation: 'Do not use this conversation final to close the current BoardTask; require post-claim durable final and task-result-artifact.',
      });
    }
  }

  const metadata = task.runtimeMetadata || task.runtime_metadata || {};
  const metadataHash =
    metadata.task_result_artifact_hash ||
    metadata.result_artifact_hash ||
    metadata.artifact_hash ||
    null;
  const summaryNotes = (Array.isArray(notes) ? notes : [])
    .filter((note) => String(note.noteType || note.note_type || '').toLowerCase() === 'summary');
  const summaryArtifactHashes = summaryNotes
    .map((note) => extractArtifactHash(note.content || ''))
    .filter(Boolean);
  if (isTerminal(task.status) && !metadataHash && summaryArtifactHashes.length === 0) {
    findings.push({
      code: 'TERMINAL_TASK_WITHOUT_ARTIFACT_HASH',
      status: task.status,
      recommendation: 'Terminal BoardTask should reference a task-result-artifact hash in runtime_metadata or summary projection.',
    });
  }

  return {
    task_id: task.id,
    title: task.title,
    status: task.status,
    claimed_at: task.claimedAt || task.claimed_at || null,
    findings,
  };
}

function extractArtifactHash(text) {
  const match = String(text || '').match(/task_result_artifact:\s*`?([a-f0-9]{16,}|sha256:[a-f0-9]{16,})`?/i);
  return match ? match[1] : null;
}

function isTerminal(status) {
  return ['done', 'failed', 'blocked', 'skipped'].includes(String(status || '').toLowerCase());
}

function dateMs(value) {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function stringArg(name) {
  const idx = args.indexOf(name);
  if (idx === -1 || idx + 1 >= args.length) return '';
  return args[idx + 1] || '';
}

function numberArg(name, fallback) {
  const idx = args.indexOf(name);
  if (idx === -1 || idx + 1 >= args.length) return fallback;
  const value = Number(args[idx + 1]);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

async function callTool(name, toolArgs = {}) {
  const result = await callMissiond('tools/call', { name, arguments: toolArgs }, timeoutMs);
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

function finish(result, code) {
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(`Stale BoardTask final audit OK (${result.scanned_tasks} task(s) scanned).`);
  } else {
    console.error(JSON.stringify(result, null, 2));
  }
  process.exit(code);
}
