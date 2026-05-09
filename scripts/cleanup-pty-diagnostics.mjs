#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const DEFAULT_DAYS = 1;
const DEFAULT_DB = process.env.MISSION_PG_DATABASE || 'missiond';

function parseArgs(argv) {
  const opts = {
    days: DEFAULT_DAYS,
    apply: false,
    db: DEFAULT_DB,
    skipFiles: false,
    skipDb: false,
    vacuum: false,
    vacuumFull: false,
    json: false,
    missionHome: process.env.MISSION_HOME || path.join(os.homedir(), '.xjp-mission'),
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--days') opts.days = Number(argv[++i]);
    else if (arg === '--db') opts.db = argv[++i];
    else if (arg === '--mission-home') opts.missionHome = argv[++i];
    else if (arg === '--skip-files') opts.skipFiles = true;
    else if (arg === '--skip-db') opts.skipDb = true;
    else if (arg === '--vacuum') opts.vacuum = true;
    else if (arg === '--vacuum-full') opts.vacuumFull = true;
    else if (arg === '--apply') opts.apply = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/cleanup-pty-diagnostics.mjs [--days 1] [--db missiond] [--mission-home ~/.xjp-mission] [--skip-files] [--skip-db] [--vacuum] [--vacuum-full] [--apply] [--json]');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  if (!Number.isFinite(opts.days) || opts.days < 0) {
    throw new Error(`--days must be a non-negative number, got ${opts.days}`);
  }
  opts.missionHome = path.resolve(opts.missionHome.replace(/^~(?=$|\/)/, os.homedir()));
  return opts;
}

function psql(db, sql) {
  const result = spawnSync('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1', '-At', '-c', sql], {
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 100,
  });
  if (result.status !== 0) {
    throw new Error(`psql failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
  return result.stdout.trim();
}

function psqlJson(db, sql) {
  const out = psql(db, `SELECT COALESCE(row_to_json(t), '{}'::json) FROM (${sql}) t`);
  return JSON.parse(out || '{}');
}

function sqlLiteral(value) {
  return `'${String(value).replace(/'/g, "''")}'`;
}

function walk(dir) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else if (entry.isFile()) out.push(full);
  }
  return out;
}

function isPtyDiagnosticFile(file, missionHome) {
  const rel = path.relative(missionHome, file);
  if (rel.startsWith('..') || path.isAbsolute(rel)) return false;
  const parts = rel.split(path.sep);
  if (parts[0] === 'logs') {
    return /^pty-.*\.log$/.test(path.basename(file));
  }
  if (parts[0] === 'screenshots') {
    return /\.(png|jpg|jpeg|webp)$/i.test(path.basename(file));
  }
  return false;
}

function collect(opts) {
  const cutoffMs = Date.now() - opts.days * 24 * 60 * 60 * 1000;
  const roots = [path.join(opts.missionHome, 'logs'), path.join(opts.missionHome, 'screenshots')];
  const candidates = roots.flatMap(walk)
    .filter((file) => isPtyDiagnosticFile(file, opts.missionHome))
    .map((file) => {
      const stat = fs.statSync(file);
      return {
        path: file,
        size: stat.size,
        mtime: stat.mtime.toISOString(),
        expired: stat.mtime.getTime() < cutoffMs,
      };
    });
  return {
    cutoff: new Date(cutoffMs).toISOString(),
    candidates,
    expired: candidates.filter((item) => item.expired),
  };
}

function collectDb(opts) {
  const cutoff = new Date(Date.now() - opts.days * 24 * 60 * 60 * 1000).toISOString();
  const cutoffSql = `${sqlLiteral(cutoff)}::timestamptz`;
  const sql = `
    WITH pty_session_stats AS (
      SELECT
        c.id,
        COUNT(m.id)::bigint AS message_count,
        MIN(m.timestamp) AS min_message_at,
        MAX(m.timestamp) AS max_message_at
      FROM conversations c
      LEFT JOIN conversation_messages m ON m.session_id = c.id
      WHERE COALESCE(c.chat_type, '') = 'pty'
      GROUP BY c.id
    ),
    expired_sessions AS (
      SELECT *
      FROM pty_session_stats
      WHERE message_count > 0
        AND max_message_at < ${cutoffSql}
    ),
    expired_messages AS (
      SELECT m.id
      FROM conversation_messages m
      JOIN expired_sessions s ON s.id = m.session_id
    )
    SELECT
      ${sqlLiteral(cutoff)} AS cutoff,
      COALESCE((SELECT COUNT(*) FROM expired_sessions), 0)::bigint AS expired_sessions,
      COALESCE((SELECT SUM(message_count) FROM expired_sessions), 0)::bigint AS expired_messages,
      COALESCE((SELECT COUNT(*) FROM conversation_turns t JOIN expired_sessions s ON s.id = t.session_id), 0)::bigint AS expired_turns,
      COALESCE((SELECT COUNT(*) FROM message_labels l JOIN expired_messages m ON m.id = l.message_id), 0)::bigint AS expired_labels,
      COALESCE((SELECT COUNT(*) FROM conversation_tool_calls tc JOIN expired_sessions s ON s.id = tc.session_id), 0)::bigint AS expired_tool_calls,
      COALESCE((SELECT COUNT(*) FROM conversation_events e JOIN expired_sessions s ON s.id = e.session_id), 0)::bigint AS expired_events,
      COALESCE((SELECT COUNT(*) FROM conversation_topic_vectors v JOIN expired_sessions s ON s.id = v.session_id), 0)::bigint AS expired_topic_vectors,
      COALESCE((SELECT COUNT(*) FROM message_embeddings e JOIN expired_messages m ON m.id = e.message_id), 0)::bigint AS expired_embeddings,
      COALESCE((SELECT COUNT(*) FROM message_embedding_skips s JOIN expired_messages m ON m.id = s.message_id), 0)::bigint AS expired_embedding_skips,
      COALESCE((SELECT MIN(min_message_at) FROM expired_sessions)::text, '') AS min_message_at,
      COALESCE((SELECT MAX(max_message_at) FROM expired_sessions)::text, '') AS max_message_at
  `;
  return psqlJson(opts.db, sql);
}

function applyDbCleanup(opts) {
  const cutoff = new Date(Date.now() - opts.days * 24 * 60 * 60 * 1000).toISOString();
  const cutoffSql = `${sqlLiteral(cutoff)}::timestamptz`;
  const sql = `
    BEGIN;
    CREATE TEMP TABLE expired_pty_sessions ON COMMIT DROP AS
      WITH pty_session_stats AS (
        SELECT c.id, COUNT(m.id)::bigint AS message_count, MAX(m.timestamp) AS max_message_at
        FROM conversations c
        LEFT JOIN conversation_messages m ON m.session_id = c.id
        WHERE COALESCE(c.chat_type, '') = 'pty'
        GROUP BY c.id
      )
      SELECT id
      FROM pty_session_stats
      WHERE message_count > 0
        AND max_message_at < ${cutoffSql};

    CREATE TEMP TABLE expired_pty_messages ON COMMIT DROP AS
      SELECT m.id
      FROM conversation_messages m
      JOIN expired_pty_sessions s ON s.id = m.session_id;

    WITH
      del_turns AS (
        DELETE FROM conversation_turns t
        USING expired_pty_sessions s
        WHERE t.session_id = s.id
        RETURNING 1
      ),
      del_labels AS (
        DELETE FROM message_labels l
        USING expired_pty_messages m
        WHERE l.message_id = m.id
        RETURNING 1
      ),
      del_tool_calls AS (
        DELETE FROM conversation_tool_calls tc
        USING expired_pty_sessions s
        WHERE tc.session_id = s.id
        RETURNING 1
      ),
      del_events AS (
        DELETE FROM conversation_events e
        USING expired_pty_sessions s
        WHERE e.session_id = s.id
        RETURNING 1
      ),
      del_topic_vectors AS (
        DELETE FROM conversation_topic_vectors v
        USING expired_pty_sessions s
        WHERE v.session_id = s.id
        RETURNING 1
      ),
      del_embeddings AS (
        DELETE FROM message_embeddings e
        USING expired_pty_messages m
        WHERE e.message_id = m.id
        RETURNING 1
      ),
      del_embedding_skips AS (
        DELETE FROM message_embedding_skips e
        USING expired_pty_messages m
        WHERE e.message_id = m.id
        RETURNING 1
      ),
      del_messages AS (
        DELETE FROM conversation_messages msg
        USING expired_pty_sessions s
        WHERE msg.session_id = s.id
        RETURNING 1
      ),
      del_conversations AS (
        DELETE FROM conversations c
        USING expired_pty_sessions s
        WHERE c.id = s.id
        RETURNING 1
      )
    SELECT json_build_object(
      'turns', (SELECT COUNT(*) FROM del_turns),
      'labels', (SELECT COUNT(*) FROM del_labels),
      'tool_calls', (SELECT COUNT(*) FROM del_tool_calls),
      'events', (SELECT COUNT(*) FROM del_events),
      'topic_vectors', (SELECT COUNT(*) FROM del_topic_vectors),
      'embeddings', (SELECT COUNT(*) FROM del_embeddings),
      'embedding_skips', (SELECT COUNT(*) FROM del_embedding_skips),
      'messages', (SELECT COUNT(*) FROM del_messages),
      'conversations', (SELECT COUNT(*) FROM del_conversations)
    );
    COMMIT;
  `;
  const out = spawnSync('psql', ['-d', opts.db, '-v', 'ON_ERROR_STOP=1'], {
    input: sql,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 100,
  });
  if (out.status !== 0) {
    throw new Error(`psql cleanup failed (${out.status})\nSTDOUT:\n${out.stdout}\nSTDERR:\n${out.stderr}`);
  }
  const jsonLine = out.stdout.split('\n').find((line) => line.trim().startsWith('{'));
  return jsonLine ? JSON.parse(jsonLine) : {};
}

function vacuumDb(opts) {
  const mode = opts.vacuumFull ? 'VACUUM (FULL, ANALYZE)' : 'VACUUM (ANALYZE)';
  const tables = [
    'conversation_messages',
    'message_labels',
    'conversation_tool_calls',
    'conversation_events',
    'conversation_turns',
    'conversation_topic_vectors',
    'conversations',
  ];
  for (const table of tables) {
    psql(opts.db, `${mode} ${table};`);
  }
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const result = opts.skipFiles ? { cutoff: null, candidates: [], expired: [] } : collect(opts);
  const dbBefore = opts.skipDb ? null : collectDb(opts);
  let removed = 0;
  let removedBytes = 0;
  let dbRemoved = null;
  if (opts.apply && !opts.skipFiles) {
    for (const item of result.expired) {
      fs.unlinkSync(item.path);
      removed += 1;
      removedBytes += item.size;
    }
  }
  if (opts.apply && !opts.skipDb) {
    dbRemoved = applyDbCleanup(opts);
    if (opts.vacuum || opts.vacuumFull) vacuumDb(opts);
  }
  const dbAfter = opts.skipDb ? null : collectDb(opts);
  const summary = {
    ok: true,
    mode: opts.apply ? 'apply' : 'dry-run',
    mission_home: opts.missionHome,
    db: opts.db,
    retention_days: opts.days,
    cutoff: result.cutoff,
    files: {
      skipped: opts.skipFiles,
      candidates: result.candidates.length,
      expired: result.expired.length,
      expired_bytes: result.expired.reduce((sum, item) => sum + item.size, 0),
      removed,
      removed_bytes: removedBytes,
      expired_files: result.expired.map((item) => item.path),
    },
    database: {
      skipped: opts.skipDb,
      before: dbBefore,
      removed: dbRemoved,
      after: dbAfter,
      vacuum: opts.apply && (opts.vacuum || opts.vacuumFull) ? (opts.vacuumFull ? 'full' : 'analyze') : 'not-run',
    },
  };
  if (opts.json) {
    console.log(JSON.stringify(summary, null, 2));
  } else {
    console.log(`PTY diagnostics cleanup ${summary.mode}`);
    console.log(`mission_home: ${summary.mission_home}`);
    console.log(`db: ${summary.db}`);
    console.log(`retention_days: ${summary.retention_days}`);
    console.log(`file expired: ${summary.files.expired} file(s), ${summary.files.expired_bytes} byte(s)`);
    if (summary.database.before) {
      console.log(`db expired: ${summary.database.before.expired_sessions} session(s), ${summary.database.before.expired_messages} message(s)`);
    }
    if (opts.apply) {
      console.log(`removed files: ${summary.files.removed} file(s), ${summary.files.removed_bytes} byte(s)`);
      if (summary.database.removed) console.log(`removed db: ${JSON.stringify(summary.database.removed)}`);
    }
    for (const file of summary.files.expired_files) console.log(file);
  }
}

main();
