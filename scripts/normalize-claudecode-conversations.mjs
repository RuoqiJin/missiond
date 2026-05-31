#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const DEFAULT_DB = process.env.MISSION_PG_DATABASE || 'missiond';
const DEFAULT_CLAUDE_PROJECTS = path.join(os.homedir(), '.claude', 'projects');

function parseArgs(argv) {
  const opts = {
    db: DEFAULT_DB,
    claudeProjects: DEFAULT_CLAUDE_PROJECTS,
    apply: false,
    json: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--db') opts.db = argv[++i];
    else if (arg === '--claude-projects') opts.claudeProjects = argv[++i];
    else if (arg === '--apply') opts.apply = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/normalize-claudecode-conversations.mjs [--db missiond] [--claude-projects ~/.claude/projects] [--apply] [--json]');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  opts.claudeProjects = path.resolve(opts.claudeProjects.replace(/^~(?=$|\/)/, os.homedir()));
  return opts;
}

function run(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, {
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 200,
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${cmd} failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
  return result.stdout;
}

function psql(db, sql) {
  return run('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1', '-At', '-F', '\t', '-c', sql]);
}

function psqlJson(db, sql) {
  const out = psql(db, `SELECT COALESCE(json_agg(t), '[]'::json) FROM (${sql}) t`).trim();
  return JSON.parse(out || '[]');
}

function listJsonlFiles(root) {
  if (!fs.existsSync(root)) return [];
  return run('find', [
    root,
    '-name',
    '*.jsonl',
    '!',
    '-path',
    '*tool-results*',
    '!',
    '-path',
    '*session-memory*',
    '-print',
  ]).split('\n').filter(Boolean);
}

function extractText(value) {
  const content = value?.message?.content ?? value?.content ?? '';
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) {
    return content.map((part) => {
      if (typeof part === 'string') return part;
      if (part && typeof part === 'object') return String(part.text ?? part.content ?? part.type ?? '');
      return '';
    }).filter(Boolean).join('\n');
  }
  return JSON.stringify(content);
}

function isLocalCommandLike(text) {
  const t = String(text || '').trim();
  return t.startsWith('<local-command-')
    || t.startsWith('<command-name>')
    || t.startsWith('<command-message>')
    || t.startsWith('<command-args>')
    || t.startsWith('[Request interrupted')
    || t.startsWith('(Bash completed')
    || t.includes('<local-command-stdout>');
}

function scanRawFiles(root) {
  const rows = [];
  const rawIds = new Set();
  const skippedLocalOnly = [];
  for (const file of listJsonlFiles(root)) {
    const sessionId = path.basename(file, '.jsonl');
    rawIds.add(sessionId);
    const text = fs.readFileSync(file, 'utf8');
    const hash = crypto.createHash('sha256').update(text).digest('hex');
    const lines = text.split('\n').filter((line) => line.trim());
    let messageLines = 0;
    let first = null;
    let last = null;
    let hasAssistant = false;
    let hasMessage = false;
    let onlyLocalCommandLike = true;
    for (const line of lines) {
      let value;
      try {
        value = JSON.parse(line);
      } catch {
        continue;
      }
      const ts = value.timestamp || value.created_at || value.createdAt || value.time;
      if (ts) {
        const iso = new Date(ts).toISOString();
        if (!first || iso < first) first = iso;
        if (!last || iso > last) last = iso;
      }
      const role = value?.message?.role ?? value?.type;
      if (role === 'user' || role === 'assistant') {
        hasMessage = true;
        messageLines += 1;
        if (role === 'assistant') {
          hasAssistant = true;
          onlyLocalCommandLike = false;
        } else if (!isLocalCommandLike(extractText(value))) {
          onlyLocalCommandLike = false;
        }
      }
    }
    rows.push({
      sessionId,
      rawPath: file,
      rawLineCount: lines.length,
      rawMessageLineCount: messageLines,
      rawHash: hash,
      rawFirstSeenAt: first,
      rawLastSeenAt: last,
      hasAssistant,
      localCommandOnly: hasMessage && onlyLocalCommandLike,
    });
    if (hasMessage && onlyLocalCommandLike) {
      skippedLocalOnly.push({ session_id: sessionId, raw_path: file, raw_line_count: lines.length });
    }
  }
  return { rows, rawIds, skippedLocalOnly };
}

function writeRawScanTsv(rows) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'claude-source-state-'));
  const file = path.join(tmp, 'raw.tsv');
  const tsv = rows.map((row) => [
    row.sessionId,
    row.rawPath,
    row.rawLineCount,
    row.rawMessageLineCount,
    row.rawHash,
    row.rawFirstSeenAt ?? '',
    row.rawLastSeenAt ?? '',
    row.hasAssistant ? 'true' : 'false',
    row.localCommandOnly ? 'true' : 'false',
  ].map((v) => String(v).replaceAll('\\', '\\\\').replaceAll('\t', '\\t').replaceAll('\n', '\\n')).join('\t')).join('\n');
  fs.writeFileSync(file, `${tsv}\n`);
  return { tmp, file };
}

function quoteLiteral(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

function applySourceState(db, rawRows) {
  const { tmp, file } = writeRawScanTsv(rawRows);
  const sql = `
    CREATE TABLE IF NOT EXISTS conversation_source_state (
      conversation_id text PRIMARY KEY,
      source text NOT NULL,
      raw_path text,
      raw_state text NOT NULL,
      raw_first_seen_at timestamptz,
      raw_last_seen_at timestamptz,
      raw_line_count bigint,
      raw_message_line_count bigint,
      raw_hash text,
      reason text,
      updated_at timestamptz NOT NULL DEFAULT now()
    );

    CREATE TEMP TABLE tmp_claude_raw_scan (
      session_id text,
      raw_path text,
      raw_line_count bigint,
      raw_message_line_count bigint,
      raw_hash text,
      raw_first_seen_at timestamptz,
      raw_last_seen_at timestamptz,
      has_assistant boolean,
      local_command_only boolean
    );
    \\copy tmp_claude_raw_scan FROM ${quoteLiteral(file)} WITH (FORMAT text, NULL '')

    INSERT INTO conversation_source_state (
      conversation_id, source, raw_path, raw_state, raw_first_seen_at, raw_last_seen_at,
      raw_line_count, raw_message_line_count, raw_hash, reason, updated_at
    )
    SELECT
      c.id,
      c.source,
      COALESCE(NULLIF(c.jsonl_path, ''), r.raw_path),
      CASE
        WHEN c.conversation_type = 'history_prompt' AND c.chat_type = 'history_jsonl' THEN 'current'
        WHEN COALESCE(c.jsonl_path, '') = '' THEN 'unknown'
        WHEN r.session_id IS NOT NULL AND r.raw_path = c.jsonl_path THEN
          CASE
            WHEN r.local_command_only AND NOT r.has_assistant THEN 'raw-only-local-command'
            WHEN NOT r.has_assistant THEN 'raw-only-provider-prompt'
            ELSE 'current'
          END
        WHEN r.session_id IS NOT NULL AND r.raw_path <> c.jsonl_path THEN 'path-mismatch'
        ELSE 'missing-stale'
      END AS raw_state,
      CASE WHEN c.conversation_type = 'history_prompt' THEN
        (SELECT MIN(m.timestamp)::timestamptz FROM conversation_messages m WHERE m.session_id = c.id)
      ELSE r.raw_first_seen_at END,
      CASE WHEN c.conversation_type = 'history_prompt' THEN
        (SELECT MAX(m.timestamp)::timestamptz FROM conversation_messages m WHERE m.session_id = c.id)
      ELSE r.raw_last_seen_at END,
      CASE WHEN c.conversation_type = 'history_prompt' THEN 1 ELSE r.raw_line_count END,
      CASE WHEN c.conversation_type = 'history_prompt' THEN 1 ELSE r.raw_message_line_count END,
      CASE WHEN c.conversation_type = 'history_prompt' THEN
        (SELECT replace(m.message_uuid, 'claude-history:', '') FROM conversation_messages m WHERE m.session_id = c.id LIMIT 1)
      ELSE r.raw_hash END,
      CASE
        WHEN c.conversation_type = 'history_prompt' AND c.chat_type = 'history_jsonl' THEN 'prompt-only record from ~/.claude/history.jsonl'
        WHEN COALESCE(c.jsonl_path, '') = '' THEN 'conversation has no raw path'
        WHEN r.session_id IS NOT NULL AND r.raw_path = c.jsonl_path AND r.local_command_only AND NOT r.has_assistant THEN 'raw file contains only local command/no assistant material'
        WHEN r.session_id IS NOT NULL AND r.raw_path = c.jsonl_path AND NOT r.has_assistant THEN 'raw file contains no assistant transcript material'
        WHEN r.session_id IS NOT NULL AND r.raw_path = c.jsonl_path THEN 'raw path exists and matches current scan'
        WHEN r.session_id IS NOT NULL AND r.raw_path <> c.jsonl_path THEN 'same session id found at a different path'
        ELSE 'recorded raw path is not present in current ~/.claude/projects scan'
      END,
      now()
    FROM conversations c
    LEFT JOIN (
      SELECT DISTINCT ON (session_id) *
      FROM tmp_claude_raw_scan
      ORDER BY session_id, raw_path
    ) r ON r.session_id = c.id
    WHERE c.source = 'claude_code'
    ON CONFLICT (conversation_id) DO UPDATE SET
      source = EXCLUDED.source,
      raw_path = EXCLUDED.raw_path,
      raw_state = EXCLUDED.raw_state,
      raw_first_seen_at = EXCLUDED.raw_first_seen_at,
      raw_last_seen_at = EXCLUDED.raw_last_seen_at,
      raw_line_count = EXCLUDED.raw_line_count,
      raw_message_line_count = EXCLUDED.raw_message_line_count,
      raw_hash = EXCLUDED.raw_hash,
      reason = EXCLUDED.reason,
      updated_at = now();

    INSERT INTO conversation_source_state (
      conversation_id, source, raw_path, raw_state, raw_first_seen_at, raw_last_seen_at,
      raw_line_count, raw_message_line_count, raw_hash, reason, updated_at
    )
    SELECT
      r.session_id,
      'claude_code',
      r.raw_path,
      CASE
        WHEN r.local_command_only AND NOT r.has_assistant THEN 'raw-only-local-command'
        WHEN NOT r.has_assistant THEN 'raw-only-provider-prompt'
        ELSE 'raw-only-uningested'
      END,
      r.raw_first_seen_at,
      r.raw_last_seen_at,
      r.raw_line_count,
      r.raw_message_line_count,
      r.raw_hash,
      CASE
        WHEN r.local_command_only AND NOT r.has_assistant THEN 'raw file exists but contains only local-command/no-assistant material, so it is tracked but not imported as a conversation'
        WHEN NOT r.has_assistant THEN 'raw file exists but contains only provider sidechain/user prompt material, so it is tracked as source evidence instead of a full conversation'
        ELSE 'raw file exists but no conversation row has been ingested yet'
      END,
      now()
    FROM (
      SELECT DISTINCT ON (session_id) *
      FROM tmp_claude_raw_scan
      ORDER BY session_id, raw_path
    ) r
    LEFT JOIN conversations c ON c.source='claude_code' AND c.id = r.session_id
    WHERE c.id IS NULL
    ON CONFLICT (conversation_id) DO UPDATE SET
      source = EXCLUDED.source,
      raw_path = EXCLUDED.raw_path,
      raw_state = EXCLUDED.raw_state,
      raw_first_seen_at = EXCLUDED.raw_first_seen_at,
      raw_last_seen_at = EXCLUDED.raw_last_seen_at,
      raw_line_count = EXCLUDED.raw_line_count,
      raw_message_line_count = EXCLUDED.raw_message_line_count,
      raw_hash = EXCLUDED.raw_hash,
      reason = EXCLUDED.reason,
      updated_at = now();
  `;
  const result = spawnSync('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1'], {
    input: sql,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 200,
  });
  fs.rmSync(tmp, { recursive: true, force: true });
  if (result.status !== 0) {
    throw new Error(`source-state psql failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
}

function applyMessageLabels(db) {
  const sql = `
    WITH scoped AS (
      SELECT m.id, m.session_id, m.message_uuid, m.role, m.raw_role, m.timestamp, md5(m.content) AS content_hash,
             ROW_NUMBER() OVER (
               PARTITION BY
                 m.session_id,
                 m.role,
                 COALESCE(m.raw_role, ''),
                 m.timestamp,
                 md5(m.content)
               ORDER BY m.id
             ) AS rn
      FROM conversations c
      JOIN conversation_messages m ON m.session_id = c.id
      WHERE c.source = 'claude_code'
    ), labels AS (
      SELECT id AS message_id, 'canonical_state' AS label,
             CASE WHEN rn = 1 THEN 'canonical' ELSE 'equivalent-duplicate' END AS value,
             'claudecode-normalizer' AS source
      FROM scoped
      UNION ALL
      SELECT m.id, 'raw_role_state',
             CASE
               WHEN m.raw_role IS NOT NULL THEN 'native'
               WHEN m.role IN ('assistant', 'agent_assistant', 'worker_user', 'agent_user') THEN 'reconstructed'
               WHEN m.role IN ('thinking', 'tool_result', 'compact_summary') THEN 'provider-derived'
               ELSE 'ambiguous'
             END,
             'claudecode-normalizer'
      FROM conversations c
      JOIN conversation_messages m ON m.session_id = c.id
      WHERE c.source = 'claude_code'
    )
    INSERT INTO message_labels (message_id, label, value, source)
    SELECT message_id, label, value, source FROM labels
    ON CONFLICT (message_id, label) DO UPDATE SET
      value = EXCLUDED.value,
      source = EXCLUDED.source;
  `;
  psql(db, sql);
}

function stats(db, skippedLocalOnly) {
  const hasSourceState = psql(db, "SELECT to_regclass('public.conversation_source_state') IS NOT NULL").trim() === 't';
  return {
    source_state: hasSourceState ? psqlJson(db, `
      SELECT raw_state, COUNT(*)::int AS conversations, COALESCE(SUM(raw_message_line_count),0)::int AS raw_message_lines
      FROM conversation_source_state
      WHERE source='claude_code'
      GROUP BY 1
      ORDER BY 1
    `) : [],
    canonical_labels: psqlJson(db, `
      SELECT value, COUNT(*)::int AS messages
      FROM message_labels ml
      JOIN conversation_messages m ON m.id=ml.message_id
      JOIN conversations c ON c.id=m.session_id
      WHERE c.source='claude_code' AND ml.label='canonical_state'
      GROUP BY 1 ORDER BY 1
    `),
    raw_role_state: psqlJson(db, `
      SELECT value, COUNT(*)::int AS messages
      FROM message_labels ml
      JOIN conversation_messages m ON m.id=ml.message_id
      JOIN conversations c ON c.id=m.session_id
      WHERE c.source='claude_code' AND ml.label='raw_role_state'
      GROUP BY 1 ORDER BY 1
    `),
    skipped_local_only_raw_files: skippedLocalOnly.length,
    skipped_local_only_sample: skippedLocalOnly.slice(0, 10),
  };
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const raw = scanRawFiles(opts.claudeProjects);
  const before = stats(opts.db, raw.skippedLocalOnly);
  if (opts.apply) {
    applySourceState(opts.db, raw.rows);
    applyMessageLabels(opts.db);
  }
  const after = opts.apply ? stats(opts.db, raw.skippedLocalOnly) : null;
  const result = {
    ok: true,
    mode: opts.apply ? 'apply' : 'dry-run',
    db: opts.db,
    claude_projects: opts.claudeProjects,
    raw_files_scanned: raw.rows.length,
    skipped_local_only_raw_files: raw.skippedLocalOnly.length,
    before,
    after,
  };
  console.log(JSON.stringify(result, null, 2));
}

main();
