#!/usr/bin/env node
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const repoRoot = process.cwd();
const args = new Map();
for (let i = 2; i < process.argv.length; i += 1) {
  const arg = process.argv[i];
  if (!arg.startsWith('--')) continue;
  const key = arg.slice(2);
  const next = process.argv[i + 1];
  if (next && !next.startsWith('--')) {
    args.set(key, next);
    i += 1;
  } else {
    args.set(key, 'true');
  }
}

const manifestPath = path.resolve(
  repoRoot,
  args.get('manifest') ?? '.missiond/research/memory-review-v2/manifest.json',
);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const dryRun = args.get('apply') !== 'true';
const maxBodyChars = Number(args.get('max-body-chars') ?? 24000);
const retryManifestPath = path.resolve(
  repoRoot,
  args.get('retry-manifest') ?? `${manifest.output_dir}/retry-manifest.json`,
);

function sqlString(value) {
  return `'${String(value ?? '').replaceAll("'", "''")}'`;
}

function psqlJson(query) {
  const result = spawnSync('psql', ['-d', 'missiond', '-t', '-A', '-c', query], {
    encoding: 'utf8',
    maxBuffer: 512 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || `psql failed with ${result.status}`);
  }
  return JSON.parse(result.stdout.trim() || '[]');
}

function psqlExec(query) {
  const result = spawnSync('psql', ['-d', 'missiond', '-v', 'ON_ERROR_STOP=1', '-q', '-c', query], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || `psql failed with ${result.status}`);
  }
}

function sha256Hex(input) {
  return crypto.createHash('sha256').update(input).digest('hex');
}

function redactSecrets(text) {
  return String(text ?? '')
    .replace(/\b(XJP_[A-Za-z0-9_-]{12,})\b/g, '[REDACTED_SECRET]')
    .replace(/\b(sk-[A-Za-z0-9_-]{12,})\b/g, '[REDACTED_SECRET]')
    .replace(/\b(Bearer\s+)[A-Za-z0-9._~+/=-]{12,}/gi, '$1[REDACTED_SECRET]')
    .replace(
      /\b(api[_-]?key|access[_-]?token|refresh[_-]?token|password|passwd|pwd|secret|client[_-]?secret)\b\s*[:=]\s*["']?[^"'\s`]+/gi,
      '$1=[REDACTED_SECRET]',
    )
    .replace(/-----BEGIN [A-Z ]+PRIVATE KEY-----[\s\S]*?-----END [A-Z ]+PRIVATE KEY-----/g, '[REDACTED_PRIVATE_KEY]');
}

function normalizeBody(row) {
  const candidates = [row.summary, row.latest_note, row.conversation_final].filter(Boolean);
  const best = candidates.sort((a, b) => String(b).length - String(a).length)[0] ?? '';
  return redactSecrets(String(best).slice(0, maxBodyChars));
}

const manifestByBatch = new Map();
for (const batch of manifest.batches ?? []) {
  const match = String(batch.id).match(/memory-review-batch-(\d+)/);
  if (!match) continue;
  manifestByBatch.set(Number(match[1]), batch);
}

const rows = psqlJson(`
with latest_summary as (
  select distinct on (task_id)
    task_id,
    content,
    created_at
  from board_task_notes
  where note_type = 'summary'
  order by task_id, created_at desc
),
latest_note as (
  select distinct on (task_id)
    task_id,
    content,
    created_at
  from board_task_notes
  where note_type <> 'summary'
  order by task_id, created_at desc
),
tasks as (
  select
    id,
    title,
    status,
    assignee,
    project_id,
    created_at,
    updated_at,
    (regexp_match(title, 'memory-review-batch-([0-9]+)'))[1]::int as batch_no
  from board_tasks
  where title ilike '%memory-review-batch-%'
)
select coalesce(json_agg(row_to_json(q) order by q.batch_no, q.created_at), '[]'::json)
from (
  select
    t.*,
    latest_summary.content as summary,
    latest_summary.created_at as summary_at,
    latest_note.content as latest_note,
    latest_note.created_at as latest_note_at,
    conv.conversation_id,
    conv.conversation_status,
    conv.conversation_ended_at,
    conv.provider,
    conv.final_content as conversation_final,
    conv.final_at as conversation_final_at,
    exists(select 1 from task_result_artifacts a where a.task_id = t.id) as has_artifact
  from tasks t
  left join latest_summary on latest_summary.task_id = t.id
  left join latest_note on latest_note.task_id = t.id
  left join lateral (
    select
      c.id as conversation_id,
      c.status as conversation_status,
      c.ended_at as conversation_ended_at,
      c.source as provider,
      msg.content as final_content,
      msg.timestamp as final_at
    from conversations c
    left join lateral (
      select cm.content, cm.timestamp
      from conversation_messages cm
      where cm.session_id = c.id
        and cm.role in ('assistant', 'agent_assistant')
        and coalesce(cm.content, '') <> ''
      order by cm.timestamp desc nulls last, cm.id desc
      limit 1
    ) msg on true
    where c.task_id = t.id
    order by c.updated_at desc nulls last, c.started_at desc nulls last
    limit 1
  ) conv on true
) q;
`);

const byBatch = new Map();
for (const row of rows) {
  if (!Number.isInteger(row.batch_no)) continue;
  const existing = byBatch.get(row.batch_no);
  if (!existing || (existing.status !== 'done' && row.status === 'done')) {
    byBatch.set(row.batch_no, row);
  }
}

const summary = {
  schema: 'missiond.memory-review-result-backfill.v1',
  generated_at: new Date().toISOString(),
  dry_run: dryRun,
  manifest: path.relative(repoRoot, manifestPath),
  manifest_batch_count: manifest.batch_count,
  task_rows: rows.length,
  distinct_task_batches: byBatch.size,
  written_reports: 0,
  written_artifacts: 0,
  skipped_existing_artifacts: 0,
  blocked_batches: [],
  missing_batches: [],
  no_result_batches: [],
  duplicate_batch_rows: rows.length - byBatch.size,
};

const batchRows = new Map();
for (const row of rows) {
  if (!Number.isInteger(row.batch_no)) continue;
  batchRows.set(row.batch_no, [...(batchRows.get(row.batch_no) ?? []), row]);
}

for (let batchNo = 1; batchNo <= manifest.batch_count; batchNo += 1) {
  const batch = manifestByBatch.get(batchNo);
  const row = byBatch.get(batchNo);
  if (!row) {
    summary.missing_batches.push(batchNo);
    continue;
  }
  if (row.status !== 'done') {
    summary.blocked_batches.push({ batch_no: batchNo, task_id: row.id, status: row.status });
    continue;
  }

  const body = normalizeBody(row);
  if (!body) {
    summary.no_result_batches.push({ batch_no: batchNo, task_id: row.id, reason: 'no summary/note/final content' });
    continue;
  }

  const reportPath = path.resolve(repoRoot, batch?.report_path ?? `${manifest.output_dir}/worker-reports/memory-review-batch-${String(batchNo).padStart(4, '0')}.md`);
  const report = [
    `# ${batch?.id ?? `memory-review-batch-${String(batchNo).padStart(4, '0')}`}`,
    '',
    `- schema: missiond.memory-review-worker-report.v1`,
    `- backfilled_at: ${new Date().toISOString()}`,
    `- source_task_id: ${row.id}`,
    `- source_status: ${row.status}`,
    `- source_conversation_id: ${row.conversation_id ?? ''}`,
    `- source_provider: ${row.provider ?? 'unknown'}`,
    `- ordinal_start: ${batch?.ordinal_start ?? ''}`,
    `- ordinal_end: ${batch?.ordinal_end ?? ''}`,
    `- item_count: ${batch?.item_count ?? ''}`,
    '',
    '## Backfilled Final',
    '',
    body,
    '',
  ].join('\n');

  const artifactBody = {
    schema: 'missiond.task-result-artifact.v1',
    source: 'memory-review-backfill',
    project_id: row.project_id ?? 'missiond',
    task_id: row.id,
    batch_id: batch?.id ?? null,
    batch_no: batchNo,
    slot_id: row.assignee ?? null,
    conversation_id: row.conversation_id ?? null,
    provider: row.provider ?? 'unknown',
    result_status: 'done',
    summary: `Backfilled memory review result for batch ${String(batchNo).padStart(4, '0')}.`,
    content: {
      report_path: path.relative(repoRoot, reportPath),
      final: body,
    },
    source_task_rows: batchRows.get(batchNo)?.map((item) => ({ id: item.id, status: item.status })) ?? [],
    created_at: new Date().toISOString(),
  };
  const artifactBytes = Buffer.from(JSON.stringify(artifactBody, null, 2) + '\n');
  const hash = sha256Hex(artifactBytes);

  if (!dryRun) {
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, report);
    const metadata = {
      schema: 'missiond.task-result-artifact.v1',
      source: 'memory-review-backfill',
      batch_id: batch?.id ?? null,
      batch_no: batchNo,
      report_path: path.relative(repoRoot, reportPath),
    };
    psqlExec(`
      INSERT INTO shared_artifacts
        (hash, kind, project_id, task_id, media_type, bytes, size_bytes, metadata)
      VALUES
        (${sqlString(hash)}, 'task-result-artifact', ${sqlString(row.project_id ?? 'missiond')}, ${sqlString(row.id)},
         'application/json', decode(${sqlString(artifactBytes.toString('hex'))}, 'hex'), ${artifactBytes.length}, ${sqlString(JSON.stringify(metadata))}::jsonb)
      ON CONFLICT(hash) DO NOTHING;

      INSERT INTO task_result_artifacts
        (id, artifact_hash, project_id, task_id, slot_id, conversation_id, provider, result_status, summary)
      VALUES
        (${sqlString(crypto.randomUUID())}, ${sqlString(hash)}, ${sqlString(row.project_id ?? 'missiond')}, ${sqlString(row.id)},
         ${row.assignee ? sqlString(row.assignee) : 'NULL'}, ${row.conversation_id ? sqlString(row.conversation_id) : 'NULL'},
         ${sqlString(row.provider ?? 'unknown')}, 'done', ${sqlString(artifactBody.summary)})
      ON CONFLICT(task_id, artifact_hash)
      DO UPDATE SET summary = EXCLUDED.summary;
    `);
  }

  summary.written_reports += 1;
  if (row.has_artifact) {
    summary.skipped_existing_artifacts += 1;
  } else {
    summary.written_artifacts += 1;
  }
}

const retryManifest = {
  schema: 'missiond.memory-review-retry-manifest.v1',
  generated_at: new Date().toISOString(),
  source_manifest: path.relative(repoRoot, manifestPath),
  missing_batches: summary.missing_batches,
  blocked_batches: summary.blocked_batches,
  no_result_batches: summary.no_result_batches,
};

if (!dryRun) {
  fs.mkdirSync(path.dirname(retryManifestPath), { recursive: true });
  fs.writeFileSync(retryManifestPath, JSON.stringify(retryManifest, null, 2) + '\n');
}

console.log(JSON.stringify({ ...summary, retry_manifest: path.relative(repoRoot, retryManifestPath) }, null, 2));
