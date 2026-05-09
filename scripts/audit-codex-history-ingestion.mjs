#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
const json = args.includes('--json');
const db = valueArg('--db', process.env.MISSION_PG_DATABASE || 'missiond');
const codexDb = valueArg('--codex-db', path.join(os.homedir(), '.codex/state_5.sqlite'));

function run(cmd, argv, opts = {}) {
  const result = spawnSync(cmd, argv, {
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 200,
    ...opts,
  });
  if (result.status !== 0) {
    throw new Error(`${cmd} ${argv.join(' ')} failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
  return result.stdout;
}

function sqliteJson(sql) {
  if (!fs.existsSync(codexDb)) return [];
  const out = run('sqlite3', ['-json', codexDb, sql]).trim();
  return out ? JSON.parse(out) : [];
}

function psqlJson(sql) {
  const out = run('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1', '-At', '-c', sql]).trim();
  return out ? JSON.parse(out) : [];
}

function valueArg(name, fallback) {
  const idx = args.indexOf(name);
  if (idx === -1 || idx + 1 >= args.length) return fallback;
  return args[idx + 1];
}

const rawThreads = sqliteJson(`
SELECT id, archived, rollout_path, created_at, updated_at, cwd, model, title
FROM threads
ORDER BY updated_at DESC;
`);

const conversations = psqlJson(`
SELECT COALESCE(json_agg(row_to_json(t)), '[]'::json)
FROM (
  SELECT id, status, conversation_type, chat_type, jsonl_path, message_count, started_at, updated_at
  FROM conversations
  WHERE source = 'codex_cli'
) t;
`);

const messageDuplicateGroups = psqlJson(`
SELECT COALESCE(json_agg(row_to_json(t)), '[]'::json)
FROM (
  SELECT message_uuid, COUNT(*)::int AS count
  FROM conversation_messages m
  JOIN conversations c ON c.id = m.session_id
  WHERE c.source = 'codex_cli'
    AND m.message_uuid IS NOT NULL
  GROUP BY message_uuid
  HAVING COUNT(*) > 1
  ORDER BY COUNT(*) DESC
  LIMIT 20
) t;
`);

const nullUuidMessages = psqlJson(`
SELECT COALESCE(json_agg(row_to_json(t)), '[]'::json)
FROM (
  SELECT COUNT(*)::int AS count
  FROM conversation_messages m
  JOIN conversations c ON c.id = m.session_id
  WHERE c.source = 'codex_cli'
    AND (m.message_uuid IS NULL OR m.message_uuid = '')
) t;
`)[0]?.count || 0;

const rawById = new Map(rawThreads.map((t) => [t.id, t]));
const dbById = new Map(conversations.map((c) => [c.id, c]));
const missingInMissionD = rawThreads
  .filter((t) => !dbById.has(t.id))
  .map((t) => pickThread(t));
const extraInMissionD = conversations
  .filter((c) => !rawById.has(c.id) && !String(c.id).startsWith('pty-slot-'))
  .map((c) => ({ id: c.id, status: c.status, conversation_type: c.conversation_type }));
const archivedStateDrift = rawThreads
  .filter((t) => Number(t.archived) === 1)
  .map((t) => ({ raw: t, db: dbById.get(t.id) }))
  .filter(({ db }) => !db || db.status !== 'archived')
  .map(({ raw, db }) => ({
    id: raw.id,
    raw_archived: raw.archived,
    db_status: db?.status || null,
    db_conversation_type: db?.conversation_type || null,
  }));
const missingRolloutFiles = rawThreads
  .filter((t) => t.rollout_path && !fs.existsSync(t.rollout_path))
  .map((t) => pickThread(t));
const rolloutJsonlFiles = countRolloutFiles();

const result = {
  ok: missingInMissionD.length === 0
    && archivedStateDrift.length === 0
    && messageDuplicateGroups.length === 0
    && nullUuidMessages === 0,
  codexDb,
  missionDb: db,
  raw: {
    threads: rawThreads.length,
    archivedThreads: rawThreads.filter((t) => Number(t.archived) === 1).length,
    rolloutJsonlFiles,
    missingRolloutFiles: missingRolloutFiles.length,
  },
  missiond: {
    conversations: conversations.length,
    placeholderConversations: conversations.filter((c) => String(c.id).startsWith('pty-slot-')).length,
    nullUuidMessages,
    duplicateUuidGroups: messageDuplicateGroups.length,
  },
  missingInMissionD,
  extraInMissionD,
  archivedStateDrift,
  duplicateUuidGroups: messageDuplicateGroups,
  missingRolloutFiles: missingRolloutFiles.slice(0, 20),
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else {
  console.log(`Codex history ingestion audit: ${result.ok ? 'OK' : 'NEEDS ATTENTION'}`);
  console.log(`- raw sqlite threads: ${result.raw.threads} (${result.raw.archivedThreads} archived)`);
  console.log(`- rollout JSONL files: ${result.raw.rolloutJsonlFiles}; missing rollout files referenced by sqlite: ${result.raw.missingRolloutFiles}`);
  console.log(`- MissionD codex conversations: ${result.missiond.conversations} (${result.missiond.placeholderConversations} PTY placeholders)`);
  console.log(`- missing in MissionD: ${result.missingInMissionD.length}`);
  console.log(`- archived state drift: ${result.archivedStateDrift.length}`);
  console.log(`- null UUID messages: ${result.missiond.nullUuidMessages}`);
  console.log(`- duplicate UUID groups: ${result.missiond.duplicateUuidGroups}`);
}

function pickThread(t) {
  return {
    id: t.id,
    archived: Number(t.archived) === 1,
    rollout_path: t.rollout_path,
    updated_at: t.updated_at,
    cwd: t.cwd,
    title: t.title,
  };
}

function countRolloutFiles() {
  const root = path.join(os.homedir(), '.codex/sessions');
  if (!fs.existsSync(root)) return 0;
  const output = run('find', [root, '-name', '*.jsonl', '-type', 'f']);
  return output.trim() ? output.trim().split('\n').length : 0;
}
