#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const DEFAULT_DB = process.env.MISSION_PG_DATABASE || 'missiond';

function parseArgs(argv) {
  const opts = {
    db: DEFAULT_DB,
    out: '',
    json: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--db') opts.db = argv[++i];
    else if (arg === '--out') opts.out = argv[++i];
    else if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/audit-claudecode-conversations.mjs [--db missiond] [--out report.md] [--json]');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
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

function claudeFiles() {
  const root = path.join(os.homedir(), '.claude', 'projects');
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
  ])
    .split('\n')
    .filter(Boolean);
}

function rawClaudeInventory() {
  const files = claudeFiles();
  let totalLines = 0;
  let messageLines = 0;
  let assistantLines = 0;
  let userLines = 0;
  let localCommandOnlySessions = 0;
  let noAssistantSessions = 0;
  const sessionIds = new Set();
  const lineCounts = new Map();

  for (const file of files) {
    const id = path.basename(file, '.jsonl');
    sessionIds.add(id);
    const text = fs.readFileSync(file, 'utf8');
    const lines = text.split('\n').filter((l) => l.trim());
    totalLines += lines.length;
    lineCounts.set(id, lines.length);
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
      const role = value?.message?.role ?? value?.type;
      const content = extractRawPreview(value);
      if (role === 'user' || role === 'assistant') {
        hasMessage = true;
        messageLines += 1;
        if (role === 'assistant') {
          assistantLines += 1;
          hasAssistant = true;
          onlyLocalCommandLike = false;
        } else if (role === 'user') {
          userLines += 1;
          if (!isLocalCommandLike(content)) onlyLocalCommandLike = false;
        }
      }
    }
    if (!hasAssistant) noAssistantSessions += 1;
    if (hasMessage && onlyLocalCommandLike) localCommandOnlySessions += 1;
  }

  return {
    files: files.length,
    sessions: sessionIds.size,
    total_lines: totalLines,
    message_lines: messageLines,
    user_lines: userLines,
    assistant_lines: assistantLines,
    no_assistant_sessions: noAssistantSessions,
    local_command_only_sessions: localCommandOnlySessions,
    session_ids: [...sessionIds].sort(),
    line_counts: lineCounts,
  };
}

function extractRawPreview(value) {
  const content = value?.message?.content ?? value?.content ?? '';
  if (typeof content === 'string') return content;
  return JSON.stringify(content).slice(0, 1000);
}

function isLocalCommandLike(text) {
  const t = String(text || '').trim();
  return (
    t.startsWith('<local-command-') ||
    t.startsWith('<command-name>') ||
    t.startsWith('<command-message>') ||
    t.startsWith('<command-args>') ||
    t.startsWith('[Request interrupted') ||
    t.startsWith('(Bash completed') ||
    t.includes('<local-command-stdout>')
  );
}

function setDiff(a, b) {
  const bSet = new Set(b);
  return a.filter((x) => !bSet.has(x));
}

function auditDb(db) {
  return {
    by_type_status: psqlJson(
      db,
      `SELECT conversation_type, COALESCE(chat_type, '') AS chat_type, status, COUNT(*)::int AS conversations, COALESCE(SUM(message_count), 0)::int AS stored_message_count
       FROM conversations
       WHERE source='claude_code'
       GROUP BY 1,2,3
       ORDER BY 1,2,3`,
    ),
    role_matrix: psqlJson(
      db,
      `SELECT conversation_type, COALESCE(chat_type, '') AS chat_type, role, COALESCE(raw_role, '') AS raw_role, COUNT(*)::int AS messages
       FROM conversations c JOIN conversation_messages m ON m.session_id=c.id
       WHERE c.source='claude_code'
       GROUP BY 1,2,3,4
       ORDER BY 1,2,3,4`,
    ),
    headline: Object.fromEntries(
      psql(db, `
        SELECT 'conversations', COUNT(*)::text FROM conversations WHERE source='claude_code'
        UNION ALL SELECT 'messages', COUNT(*)::text FROM conversations c JOIN conversation_messages m ON m.session_id=c.id WHERE c.source='claude_code'
        UNION ALL SELECT 'raw_role_null', COUNT(*)::text FROM conversations c JOIN conversation_messages m ON m.session_id=c.id WHERE c.source='claude_code' AND m.raw_role IS NULL
        UNION ALL SELECT 'message_uuid_null', COUNT(*)::text FROM conversations c JOIN conversation_messages m ON m.session_id=c.id WHERE c.source='claude_code' AND m.message_uuid IS NULL
        UNION ALL SELECT 'future_started_at', COUNT(*)::text FROM conversations WHERE source='claude_code' AND started_at ~ '^\\+?[0-9]{5,}'
        UNION ALL SELECT 'message_count_mismatch', COUNT(*)::text FROM conversations c JOIN (SELECT session_id, COUNT(*) n FROM conversation_messages GROUP BY session_id) x ON x.session_id=c.id WHERE c.source='claude_code' AND c.message_count <> x.n
        UNION ALL SELECT 'active_old_ended_null', COUNT(*)::text FROM conversations WHERE source='claude_code' AND status='active' AND ended_at IS NULL AND started_at !~ '^\\+?[0-9]{5,}' AND started_at::timestamptz < now() - interval '24 hours'
        UNION ALL SELECT 'task_bound_no_slot', COUNT(*)::text FROM conversations WHERE source='claude_code' AND COALESCE(task_id,'')<>'' AND COALESCE(slot_id,'')=''
        UNION ALL SELECT 'worker_slot_bound_no_task', COUNT(*)::text FROM conversations WHERE source='claude_code' AND conversation_type='worker' AND COALESCE(slot_id,'')<>'' AND COALESCE(task_id,'')=''
      `)
        .trim()
        .split('\n')
        .filter(Boolean)
        .map((line) => line.split('\t')),
    ),
    raw_role_null_by_role: psqlJson(
      db,
      `SELECT m.role, COUNT(*)::int AS messages
       FROM conversations c JOIN conversation_messages m ON m.session_id=c.id
       WHERE c.source='claude_code' AND m.raw_role IS NULL
       GROUP BY m.role ORDER BY COUNT(*) DESC`,
    ),
    pty_user_contamination: Object.fromEntries(
      psql(db, `
        WITH p AS (
          SELECT m.id, m.content
          FROM conversations c JOIN conversation_messages m ON m.session_id=c.id
          WHERE c.source='claude_code'
            AND c.conversation_type IN ('user','chat')
            AND c.chat_type='pty'
            AND m.role='user'
            AND COALESCE(m.raw_role,'user')='user'
        ), local_artifacts AS (
          SELECT id
          FROM p
          WHERE content ~ '^(<local-command-|<command-name>|\\[Request interrupted|\\(Bash completed|total [0-9]+ |---$)'
             OR content LIKE '%<local-command-stdout>%'
        )
        SELECT 'total', COUNT(*)::text FROM p
        UNION ALL SELECT 'local_command_or_terminal_artifact', COUNT(*)::text FROM local_artifacts
        UNION ALL SELECT 'local_command_or_terminal_artifact_unlabeled', COUNT(*)::text
          FROM local_artifacts la
          WHERE NOT EXISTS (
            SELECT 1
            FROM message_labels ml
            WHERE ml.message_id = la.id
              AND ml.source IN ('claudecode-origin-labeler', 'message_labeler')
              AND (
                (ml.label = 'origin_layer' AND ml.value = 'local_command')
                OR (ml.label = 'speaker' AND ml.value = 'terminal_artifact')
              )
          )
        UNION ALL SELECT 'board_or_worker_prompt', COUNT(*)::text FROM p WHERE content LIKE '%BoardTask ID%' OR content LIKE '%## Swarm metadata%' OR content LIKE '%## Completion protocol%' OR content LIKE 'Execute MissionD task%'
        UNION ALL SELECT 'image_or_file_context', COUNT(*)::text FROM p WHERE content LIKE '[Image:%' OR content LIKE '[Image #%'
      `)
        .trim()
        .split('\n')
        .filter(Boolean)
        .map((line) => line.split('\t')),
    ),
    origin_labels: psqlJson(
      db,
      `SELECT label, value, source, COUNT(*)::int AS messages
       FROM message_labels
       WHERE source IN ('claudecode-origin-labeler', 'message_labeler')
       GROUP BY 1,2,3
       ORDER BY 1,2,3`,
    ),
    worker_user_role_candidates: psqlJson(
      db,
      `SELECT m.id, m.session_id, c.slot_id, c.task_id,
              CASE
                WHEN m.content LIKE '<local-command-%' THEN 'local-command'
                WHEN m.content LIKE '<command-name>%' THEN 'local-command'
                WHEN m.content LIKE 'Execute MissionD task %' THEN 'worker-prompt'
                WHEN m.content LIKE 'Implement accepted swarm shard%' THEN 'worker-prompt'
                WHEN m.content ILIKE '%BoardTask ID%' THEN 'worker-prompt'
                WHEN m.content ILIKE '%Task contract SSOT%' THEN 'worker-prompt'
                ELSE 'worker-user-raw-role'
              END AS reason,
              LEFT(regexp_replace(COALESCE(m.content, ''), '\\s+', ' ', 'g'), 200) AS preview
       FROM conversations c JOIN conversation_messages m ON m.session_id=c.id
       WHERE c.source='claude_code'
         AND c.conversation_type='worker'
         AND m.role='user'
         AND m.raw_role='user'
       ORDER BY m.id DESC
       LIMIT 50`,
    ),
    db_session_ids: psql(db, `SELECT id FROM conversations WHERE source='claude_code' AND conversation_type <> 'history_prompt' ORDER BY id`)
      .trim()
      .split('\n')
      .filter(Boolean),
    history_prompt_summary: psqlJson(
      db,
      `SELECT COUNT(*)::int AS conversations,
              MIN(m.timestamp)::text AS first_prompt_at,
              MAX(m.timestamp)::text AS last_prompt_at
       FROM conversations c
       JOIN conversation_messages m ON m.session_id=c.id
       WHERE c.source='claude_code'
         AND c.conversation_type='history_prompt'
         AND c.chat_type='history_jsonl'`,
    )[0],
    source_state_counts: psqlJson(
      db,
      `SELECT raw_state, COUNT(*)::int AS conversations, COALESCE(SUM(raw_message_line_count),0)::int AS raw_message_lines
       FROM conversation_source_state
       WHERE source='claude_code'
       GROUP BY 1
       ORDER BY 1`,
    ),
    raw_only_source_ids: psql(db, `
      SELECT conversation_id
      FROM conversation_source_state
      WHERE source='claude_code'
        AND raw_state IN ('raw-only-local-command','raw-only-provider-prompt','raw-only-uningested')
      ORDER BY conversation_id
    `)
      .trim()
      .split('\n')
      .filter(Boolean),
    missing_stale_source_ids: psql(db, `
      SELECT conversation_id
      FROM conversation_source_state
      WHERE source='claude_code'
        AND raw_state='missing-stale'
      ORDER BY conversation_id
    `)
      .trim()
      .split('\n')
      .filter(Boolean),
  };
}

function buildAudit(db) {
  const raw = rawClaudeInventory();
  const dbAudit = auditDb(db);
  const rawMissingInDb = setDiff(raw.session_ids, dbAudit.db_session_ids);
  const rawOnlyTracked = new Set(dbAudit.raw_only_source_ids);
  const rawMissingUntracked = rawMissingInDb.filter((id) => !rawOnlyTracked.has(id));
  const dbMissingInRaw = setDiff(dbAudit.db_session_ids, raw.session_ids);
  const missingStaleTracked = new Set(dbAudit.missing_stale_source_ids);
  const dbMissingUntracked = dbMissingInRaw.filter((id) => !missingStaleTracked.has(id));
  const missingDetails = rawMissingUntracked.map((id) => ({
    session_id: id,
    raw_lines: raw.line_counts.get(id) ?? 0,
  }));

  const issues = [];
  const h = dbAudit.headline;
  if (Number(h.raw_role_null ?? 0) > 0) {
    issues.push({
      severity: 'high',
      code: 'claude_raw_role_null',
      count: Number(h.raw_role_null),
      why: 'historical ClaudeCode rows cannot reliably reconstruct provider role or human/worker origin',
    });
  }
  if (Number(h.message_count_mismatch ?? 0) > 0) {
    issues.push({
      severity: 'medium',
      code: 'message_count_mismatch',
      count: Number(h.message_count_mismatch),
      why: 'conversation list/status surfaces may show stale counts',
    });
  }
  if (Number(h.future_started_at ?? 0) > 0) {
    issues.push({
      severity: 'medium',
      code: 'future_started_at',
      count: Number(h.future_started_at),
      why: 'bad timestamp parsing can break ordering and old-active detection',
    });
  }
  if (Number(dbAudit.pty_user_contamination.local_command_or_terminal_artifact_unlabeled ?? 0) > 0) {
    issues.push({
      severity: 'high',
      code: 'pty_user_contamination',
      count: Number(dbAudit.pty_user_contamination.local_command_or_terminal_artifact_unlabeled),
      why: 'ClaudeCode PTY user rows contain terminal/local-command artifacts without origin labels',
    });
  }
  if (rawMissingUntracked.length > 0) {
    issues.push({
      severity: 'low',
      code: 'raw_sessions_missing_in_db',
      count: rawMissingUntracked.length,
      why: 'some current raw JSONL sessions are not represented in conversations or source-state overlay',
    });
  }
  if (dbMissingUntracked.length > 0) {
    issues.push({
      severity: 'medium',
      code: 'db_sessions_missing_raw_file',
      count: dbMissingUntracked.length,
      why: 'historical DB conversations no longer have local raw JSONL evidence; audits need a stale-source state',
    });
  }

  return {
    generated_at: new Date().toISOString(),
    db,
    raw_inventory: {
      files: raw.files,
      sessions: raw.sessions,
      total_lines: raw.total_lines,
      message_lines: raw.message_lines,
      user_lines: raw.user_lines,
      assistant_lines: raw.assistant_lines,
      no_assistant_sessions: raw.no_assistant_sessions,
      local_command_only_sessions: raw.local_command_only_sessions,
    },
    db_inventory: {
      headline: dbAudit.headline,
      by_type_status: dbAudit.by_type_status,
      raw_role_null_by_role: dbAudit.raw_role_null_by_role,
      pty_user_contamination: dbAudit.pty_user_contamination,
      history_prompt_summary: dbAudit.history_prompt_summary,
      source_state_counts: dbAudit.source_state_counts,
      origin_labels: dbAudit.origin_labels,
      worker_user_role_candidates: dbAudit.worker_user_role_candidates,
    },
    raw_db_reconciliation: {
      raw_missing_in_db: rawMissingInDb.length,
      raw_missing_untracked: rawMissingUntracked.length,
      raw_only_tracked: rawMissingInDb.length - rawMissingUntracked.length,
      db_missing_in_raw: dbMissingInRaw.length,
      db_missing_untracked: dbMissingUntracked.length,
      db_missing_stale_tracked: dbMissingInRaw.length - dbMissingUntracked.length,
      raw_missing_sample: missingDetails.slice(0, 20),
      db_missing_sample: dbMissingInRaw.slice(0, 20),
    },
    issues,
  };
}

function renderMarkdown(audit) {
  const lines = [];
  lines.push('# ClaudeCode Conversation Management Audit');
  lines.push('');
  lines.push(`Generated at: ${audit.generated_at}`);
  lines.push(`Database: ${audit.db}`);
  lines.push('');
  lines.push('## Raw ClaudeCode JSONL Inventory');
  lines.push('');
  for (const [key, value] of Object.entries(audit.raw_inventory)) {
    lines.push(`- ${key}: ${value}`);
  }
  lines.push('');
  lines.push('## MissionD DB Inventory');
  lines.push('');
  for (const [key, value] of Object.entries(audit.db_inventory.headline)) {
    lines.push(`- ${key}: ${value}`);
  }
  lines.push('');
  lines.push('### Raw Role Null By Role');
  lines.push('');
  for (const row of audit.db_inventory.raw_role_null_by_role) {
    lines.push(`- ${row.role}: ${row.messages}`);
  }
  lines.push('');
  lines.push('### PTY User Contamination');
  lines.push('');
  for (const [key, value] of Object.entries(audit.db_inventory.pty_user_contamination)) {
    lines.push(`- ${key}: ${value}`);
  }
  lines.push('');
  lines.push('### Origin Labels Applied');
  lines.push('');
  for (const row of audit.db_inventory.origin_labels) {
    lines.push(`- ${row.label}=${row.value}: ${row.messages}`);
  }
  lines.push('');
  lines.push('## Raw / DB Reconciliation');
  lines.push('');
  lines.push(`- raw_missing_in_db: ${audit.raw_db_reconciliation.raw_missing_in_db}`);
  lines.push(`- db_missing_in_raw: ${audit.raw_db_reconciliation.db_missing_in_raw}`);
  lines.push('');
  lines.push('## Issues');
  lines.push('');
  for (const issue of audit.issues) {
    lines.push(`- [${issue.severity}] ${issue.code}: ${issue.count} — ${issue.why}`);
  }
  lines.push('');
  lines.push('## Worker User Candidate Sample');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify(audit.db_inventory.worker_user_role_candidates, null, 2));
  lines.push('```');
  lines.push('');
  return `${lines.join('\n')}\n`;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const audit = buildAudit(opts.db);
  if (opts.out) {
    fs.mkdirSync(path.dirname(opts.out), { recursive: true });
    fs.writeFileSync(opts.out, renderMarkdown(audit));
  }
  if (opts.json || !opts.out) {
    console.log(JSON.stringify(audit, null, 2));
  } else {
    console.log(JSON.stringify({
      ok: true,
      out: opts.out,
      issues: audit.issues,
      raw_db_reconciliation: audit.raw_db_reconciliation,
    }, null, 2));
  }
}

main();
