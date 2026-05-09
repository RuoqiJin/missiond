#!/usr/bin/env node
import { spawnSync } from 'node:child_process';

const DEFAULT_DB = process.env.MISSION_PG_DATABASE || 'missiond';

function parseArgs(argv) {
  const opts = { db: DEFAULT_DB, apply: false, json: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--db') opts.db = argv[++i];
    else if (arg === '--apply') opts.apply = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/label-claudecode-message-origin.mjs [--db missiond] [--apply] [--json]');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return opts;
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

function rows(db, sql) {
  return psql(db, sql)
    .trim()
    .split('\n')
    .filter((line) => line.includes('\t'))
    .map((line) => line.split('\t'));
}

const RULE_CTE = `
WITH candidates AS (
  SELECT
    m.id AS message_id,
    c.conversation_type,
    COALESCE(c.chat_type, '') AS chat_type,
    m.role,
    COALESCE(m.raw_role, '') AS raw_role,
    m.content
  FROM conversations c
  JOIN conversation_messages m ON m.session_id = c.id
  WHERE c.source = 'claude_code'
),
rules AS (
  SELECT message_id, 'origin_layer' AS label, 'local_command' AS value, 'claudecode-origin-labeler' AS source, 100 AS priority
  FROM candidates
  WHERE content ~ '^(<local-command-|<command-name>|<command-message>|<command-args>|\\[Request interrupted|\\(Bash completed)'
     OR content LIKE '%<local-command-stdout>%'

  UNION ALL
  SELECT message_id, 'speaker' AS label, 'terminal_artifact' AS value, 'claudecode-origin-labeler' AS source, 100 AS priority
  FROM candidates
  WHERE content ~ '^(<local-command-|<command-name>|<command-message>|<command-args>|\\[Request interrupted|\\(Bash completed)'
     OR content LIKE '%<local-command-stdout>%'

  UNION ALL
  SELECT message_id, 'origin_layer' AS label, 'missiond_prompt' AS value, 'claudecode-origin-labeler' AS source, 90 AS priority
  FROM candidates
  WHERE content LIKE 'Execute MissionD task %'
     OR content LIKE 'Implement accepted swarm shard%'
     OR content LIKE 'Fix MissionD-side swarm %'
     OR content LIKE 'Survey exact shards for swarm objective%'
     OR content LIKE 'Read-only smoke %'
     OR content LIKE 'Read-only MissionD %'
     OR content ILIKE '%BoardTask ID%'
     OR content ILIKE '%Task contract SSOT%'
     OR content ILIKE '%## Swarm metadata%'
     OR content ILIKE '%## Completion protocol%'
     OR content ILIKE '%write_scope%'
     OR content ILIKE '%must_not_touch%'
     OR content LIKE '有新的对话内容待分析。%'

  UNION ALL
  SELECT message_id, 'speaker' AS label, 'missiond_runtime' AS value, 'claudecode-origin-labeler' AS source, 90 AS priority
  FROM candidates
  WHERE content LIKE 'Execute MissionD task %'
     OR content LIKE 'Implement accepted swarm shard%'
     OR content LIKE 'Fix MissionD-side swarm %'
     OR content LIKE 'Survey exact shards for swarm objective%'
     OR content LIKE 'Read-only smoke %'
     OR content LIKE 'Read-only MissionD %'
     OR content ILIKE '%BoardTask ID%'
     OR content ILIKE '%Task contract SSOT%'
     OR content ILIKE '%## Swarm metadata%'
     OR content ILIKE '%## Completion protocol%'
     OR content ILIKE '%write_scope%'
     OR content ILIKE '%must_not_touch%'
     OR content LIKE '有新的对话内容待分析。%'

  UNION ALL
  SELECT message_id, 'origin_layer' AS label, 'provider_context' AS value, 'claudecode-origin-labeler' AS source, 80 AS priority
  FROM candidates
  WHERE content LIKE 'The file % has been updated successfully.%'
     OR content LIKE '<task-notification>%'
     OR content LIKE 'This session is being continued from a previous conversation%'
     OR content LIKE '[Matched Skills%'
     OR content LIKE '[Image:%'
     OR content LIKE '[Image #%'

  UNION ALL
  SELECT message_id, 'speaker' AS label, 'provider_system' AS value, 'claudecode-origin-labeler' AS source, 80 AS priority
  FROM candidates
  WHERE content LIKE 'The file % has been updated successfully.%'
     OR content LIKE '<task-notification>%'
     OR content LIKE 'This session is being continued from a previous conversation%'
     OR content LIKE '[Matched Skills%'
     OR content LIKE '[Image:%'
     OR content LIKE '[Image #%'

  UNION ALL
  SELECT message_id, 'speaker' AS label, 'worker_agent' AS value, 'claudecode-origin-labeler' AS source, 70 AS priority
  FROM candidates
  WHERE conversation_type = 'worker'
    AND role IN ('user', 'worker_user')

  UNION ALL
  SELECT message_id, 'speaker' AS label, 'subagent' AS value, 'claudecode-origin-labeler' AS source, 70 AS priority
  FROM candidates
  WHERE conversation_type = 'subagent'
    AND role IN ('user', 'agent_user')

  UNION ALL
  SELECT message_id, 'authority' AS label, 'durable_provider_log' AS value, 'claudecode-origin-labeler' AS source, 10 AS priority
  FROM candidates
  WHERE role IN ('user', 'worker_user', 'agent_user', 'assistant', 'agent_assistant', 'tool_result', 'thinking', 'compact_summary')
),
ranked AS (
  SELECT DISTINCT ON (message_id, label) message_id, label, value, source, priority
  FROM rules
  ORDER BY message_id, label, priority DESC, value
)
`;

function dryRun(db) {
  const counts = rows(
    db,
    `${RULE_CTE}
     SELECT label, value, COUNT(*)::text AS messages
     FROM ranked
     GROUP BY label, value
     ORDER BY label, value`,
  ).map(([label, value, count]) => ({ label, value, count: Number(count) }));
  const existing = rows(
    db,
    `SELECT label, value, COUNT(*)::text
     FROM message_labels
     WHERE source='claudecode-origin-labeler'
     GROUP BY label, value
     ORDER BY label, value`,
  ).map(([label, value, count]) => ({ label, value, count: Number(count) }));
  return { counts, existing };
}

function applyLabels(db) {
  const out = rows(
    db,
    `${RULE_CTE}
     INSERT INTO message_labels (message_id, label, value, source)
     SELECT message_id, label, value, source FROM ranked
     ON CONFLICT (message_id, label) DO UPDATE SET
       value = EXCLUDED.value,
       source = EXCLUDED.source
     RETURNING label, value`,
  );
  const counts = new Map();
  for (const [label, value] of out) {
    const key = `${label}\t${value}`;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([key, count]) => {
      const [label, value] = key.split('\t');
      return { label, value, count };
    })
    .sort((a, b) => `${a.label}:${a.value}`.localeCompare(`${b.label}:${b.value}`));
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const before = dryRun(opts.db);
  const applied = opts.apply ? applyLabels(opts.db) : [];
  const after = opts.apply ? dryRun(opts.db) : before;
  const result = {
    mode: opts.apply ? 'apply' : 'dry-run',
    db: opts.db,
    planned: before.counts,
    existing_before: before.existing,
    applied,
    existing_after: after.existing,
  };
  console.log(JSON.stringify(result, null, 2));
}

main();
