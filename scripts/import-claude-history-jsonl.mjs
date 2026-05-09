#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const DEFAULT_DB = process.env.MISSION_PG_DATABASE || 'missiond';
const DEFAULT_HISTORY = path.join(os.homedir(), '.claude', 'history.jsonl');

function parseArgs(argv) {
  const opts = {
    db: DEFAULT_DB,
    history: DEFAULT_HISTORY,
    apply: false,
    json: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--db') opts.db = argv[++i];
    else if (arg === '--history') opts.history = argv[++i];
    else if (arg === '--apply') opts.apply = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/import-claude-history-jsonl.mjs [--history ~/.claude/history.jsonl] [--db missiond] [--apply] [--json]');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  opts.history = path.resolve(opts.history.replace(/^~(?=$|\/)/, os.homedir()));
  return opts;
}

function sha(input) {
  return crypto.createHash('sha256').update(input).digest('hex');
}

function normalizeTimestamp(ms) {
  const n = Number(ms);
  if (!Number.isFinite(n)) return null;
  return new Date(n).toISOString();
}

function renderContent(entry) {
  const display = String(entry.display ?? '').trimEnd();
  const pasted = entry.pastedContents && typeof entry.pastedContents === 'object'
    ? Object.values(entry.pastedContents)
        .filter(Boolean)
        .sort((a, b) => Number(a.id ?? 0) - Number(b.id ?? 0))
        .map((item) => {
          const id = item.id ?? '';
          const type = item.type ?? 'unknown';
          const content = String(item.content ?? '');
          return `\n\n[Pasted content #${id} type=${type}]\n${content}`;
        })
        .join('')
    : '';
  return `${display}${pasted}`.trim();
}

function loadHistory(file) {
  const rows = [];
  const seen = new Set();
  let lineNo = 0;
  let empty = 0;
  let invalid = 0;
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    lineNo += 1;
    if (!line.trim()) continue;
    let entry;
    try {
      entry = JSON.parse(line);
    } catch {
      invalid += 1;
      continue;
    }
    const timestamp = normalizeTimestamp(entry.timestamp);
    const content = renderContent(entry);
    if (!timestamp || !content.trim()) {
      empty += 1;
      continue;
    }
    const project = String(entry.project ?? '');
    const raw = JSON.stringify(entry);
    const digest = sha(`${entry.timestamp}\0${project}\0${content}\0${lineNo}`);
    const messageUuid = `claude-history:${digest}`;
    if (seen.has(messageUuid)) continue;
    seen.add(messageUuid);
    rows.push({
      conversationId: `claude-history-${digest.slice(0, 32)}`,
      messageUuid,
      project,
      timestamp,
      startedAt: timestamp,
      content,
      raw,
      lineNo,
      historyPath: file,
    });
  }
  return { rows, empty, invalid };
}

function psql(db, sql) {
  const result = spawnSync('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1', '-At', '-F', '\t', '-c', sql], {
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 200,
  });
  if (result.status !== 0) {
    throw new Error(`psql failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
  return result.stdout;
}

function psqlJson(db, sql) {
  const out = psql(db, `SELECT COALESCE(json_agg(t), '[]'::json) FROM (${sql}) t`).trim();
  return JSON.parse(out || '[]');
}

function existingCount(db) {
  const out = psql(db, `
    SELECT COUNT(*)::text
    FROM conversations
    WHERE source='claude_code'
      AND conversation_type='history_prompt'
      AND chat_type='history_jsonl'
  `).trim();
  return Number(out || 0);
}

function quoteLiteral(value) {
  if (value === null || value === undefined) return 'NULL';
  return `'${String(value).replaceAll("'", "''")}'`;
}

function copyRowsToTemp(db, rows) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'claude-history-import-'));
  const file = path.join(tmp, 'rows.tsv');
  const lines = rows.map((row) => [
    row.conversationId,
    row.messageUuid,
    row.project,
    row.timestamp,
    row.content,
    row.raw,
    String(row.lineNo),
    row.historyPath,
  ].map((v) => String(v).replaceAll('\\', '\\\\').replaceAll('\t', '\\t').replaceAll('\n', '\\n')).join('\t'));
  fs.writeFileSync(file, `${lines.join('\n')}\n`);
  const sql = `
    CREATE TEMP TABLE tmp_claude_history_import (
      conversation_id text,
      message_uuid text,
      project text,
      ts timestamptz,
      content text,
      raw_content text,
      line_no bigint,
      history_path text
    );
    \\copy tmp_claude_history_import FROM ${quoteLiteral(file)} WITH (FORMAT text)

    INSERT INTO conversations (
      id, project, source, jsonl_path, message_count, started_at, ended_at, status,
      chat_type, conversation_type, updated_at
    )
    SELECT conversation_id, project, 'claude_code', history_path, 1,
           ts::text, ts::text, 'completed', 'history_jsonl', 'history_prompt', now()::text
    FROM tmp_claude_history_import
    ON CONFLICT (id) DO UPDATE SET
      project = EXCLUDED.project,
      jsonl_path = EXCLUDED.jsonl_path,
      message_count = 1,
      started_at = EXCLUDED.started_at,
      ended_at = EXCLUDED.ended_at,
      status = 'completed',
      chat_type = 'history_jsonl',
      conversation_type = 'history_prompt',
      updated_at = now()::text;

    INSERT INTO conversation_messages (
      session_id, role, content, raw_content, message_uuid, timestamp, raw_role, metadata
    )
    SELECT conversation_id, 'user', content, raw_content, message_uuid, ts, 'user',
           jsonb_build_object(
             'source_file', history_path,
             'source_line', line_no,
             'source_kind', 'claude_history_jsonl',
             'project', project
           )::text
    FROM tmp_claude_history_import
    ON CONFLICT (message_uuid) DO UPDATE SET
      session_id = EXCLUDED.session_id,
      role = EXCLUDED.role,
      content = EXCLUDED.content,
      raw_content = EXCLUDED.raw_content,
      timestamp = EXCLUDED.timestamp,
      raw_role = EXCLUDED.raw_role,
      metadata = EXCLUDED.metadata;

    WITH imported_messages AS (
      SELECT m.id
      FROM tmp_claude_history_import t
      JOIN conversation_messages m ON m.message_uuid = t.message_uuid
    ), labels AS (
      SELECT id AS message_id, 'authority' AS label, 'claude_history_prompt' AS value, 'claude-history-importer' AS source FROM imported_messages
      UNION ALL SELECT id, 'origin_layer', 'provider_history', 'claude-history-importer' FROM imported_messages
      UNION ALL SELECT id, 'speaker', 'human_user', 'claude-history-importer' FROM imported_messages
      UNION ALL SELECT id, 'canonical_state', 'canonical', 'claude-history-importer' FROM imported_messages
      UNION ALL SELECT id, 'raw_role_state', 'native', 'claude-history-importer' FROM imported_messages
    )
    INSERT INTO message_labels (message_id, label, value, source)
    SELECT message_id, label, value, source FROM labels
    ON CONFLICT (message_id, label) DO UPDATE SET
      value = EXCLUDED.value,
      source = EXCLUDED.source;

    WITH counts AS (
      SELECT session_id, COUNT(*)::int AS n
      FROM conversation_messages
      WHERE session_id IN (SELECT conversation_id FROM tmp_claude_history_import)
      GROUP BY session_id
    )
    UPDATE conversations c
    SET message_count = counts.n,
        updated_at = now()::text
    FROM counts
    WHERE c.id = counts.session_id;
  `;
  const result = spawnSync('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1'], {
    input: sql,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 200,
  });
  fs.rmSync(tmp, { recursive: true, force: true });
  if (result.status !== 0) {
    throw new Error(`psql import failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
}

function importStats(db) {
  return {
    conversations: Number(psql(db, `
      SELECT COUNT(*)::text
      FROM conversations
      WHERE source='claude_code'
        AND conversation_type='history_prompt'
        AND chat_type='history_jsonl'
    `).trim() || 0),
    messages: Number(psql(db, `
      SELECT COUNT(*)::text
      FROM conversations c
      JOIN conversation_messages m ON m.session_id=c.id
      WHERE c.source='claude_code'
        AND c.conversation_type='history_prompt'
        AND c.chat_type='history_jsonl'
    `).trim() || 0),
    range: psqlJson(db, `
      SELECT MIN(m.timestamp)::text AS min_timestamp, MAX(m.timestamp)::text AS max_timestamp
      FROM conversations c
      JOIN conversation_messages m ON m.session_id=c.id
      WHERE c.source='claude_code'
        AND c.conversation_type='history_prompt'
        AND c.chat_type='history_jsonl'
    `)[0],
    labels: psqlJson(db, `
      SELECT label, value, COUNT(*)::int AS messages
      FROM message_labels ml
      JOIN conversation_messages m ON m.id=ml.message_id
      JOIN conversations c ON c.id=m.session_id
      WHERE c.source='claude_code'
        AND c.conversation_type='history_prompt'
        AND c.chat_type='history_jsonl'
        AND ml.source='claude-history-importer'
      GROUP BY 1,2
      ORDER BY 1,2
    `),
  };
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const loaded = loadHistory(opts.history);
  const before = existingCount(opts.db);
  if (opts.apply) {
    copyRowsToTemp(opts.db, loaded.rows);
  }
  const after = opts.apply ? importStats(opts.db) : null;
  const result = {
    ok: true,
    mode: opts.apply ? 'apply' : 'dry-run',
    db: opts.db,
    history: opts.history,
    loaded_rows: loaded.rows.length,
    skipped_empty_or_bad_timestamp: loaded.empty,
    invalid_json_lines: loaded.invalid,
    existing_before: before,
    after,
    first_loaded_timestamp: loaded.rows[0]?.timestamp ?? null,
    last_loaded_timestamp: loaded.rows.at(-1)?.timestamp ?? null,
  };
  console.log(JSON.stringify(result, null, 2));
}

main();
