#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const repoRoot = process.cwd();
const args = new Map();
for (let i = 2; i < process.argv.length; i += 1) {
  const arg = process.argv[i];
  if (arg.startsWith('--')) {
    const key = arg.slice(2);
    const next = process.argv[i + 1];
    if (next && !next.startsWith('--')) {
      args.set(key, next);
      i += 1;
    } else {
      args.set(key, 'true');
    }
  }
}

const parentId = args.get('parent-id') ?? args.get('parentId');
if (!parentId) throw new Error('--parent-id is required');
const outPath = path.resolve(
  repoRoot,
  args.get('out') ?? `.missiond/research/memory-review/collected-${parentId.slice(0, 8)}.md`,
);
const maxFieldChars = Number(args.get('max-field-chars') ?? 20000);
const maxBodyChars = Number(args.get('max-body-chars') ?? 24000);

function sqlString(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
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

const query = `
with latest_summary as (
  select distinct on (task_id)
    task_id,
    left(content, ${maxFieldChars}) as content,
    created_at
  from board_task_notes
  where note_type = 'summary'
  order by task_id, created_at desc
),
latest_note as (
  select distinct on (task_id)
    task_id,
    left(content, ${maxFieldChars}) as content,
    created_at
  from board_task_notes
  where note_type <> 'summary'
  order by task_id, created_at desc
)
select coalesce(json_agg(row_to_json(q) order by q.created_at), '[]'::json)
from (
  select
    t.id,
    t.title,
    t.status,
    t.assignee,
    t.created_at,
    t.updated_at,
    latest_summary.content as summary,
    latest_summary.created_at as summary_at,
    latest_note.content as latest_note,
    latest_note.created_at as latest_note_at,
    conv.conversation_id,
    conv.conversation_status,
    conv.conversation_ended_at,
    conv.final_content as conversation_final,
    conv.final_at as conversation_final_at
  from board_tasks t
  left join latest_summary on latest_summary.task_id = t.id
  left join latest_note on latest_note.task_id = t.id
  left join lateral (
    select
      c.id as conversation_id,
      c.status as conversation_status,
      c.ended_at as conversation_ended_at,
      msg.content as final_content,
      msg.timestamp as final_at
    from conversations c
    left join lateral (
      select left(cm.content, ${maxFieldChars}) as content, cm.timestamp
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
  where t.parent_id = ${sqlString(parentId)}
  order by t.created_at
) q;
`;

const result = spawnSync('psql', ['-d', 'missiond', '-t', '-A', '-c', query], {
  encoding: 'utf8',
  maxBuffer: 256 * 1024 * 1024,
});
if (result.status !== 0) {
  throw new Error(result.stderr || result.stdout || `psql failed with ${result.status}`);
}
const rows = JSON.parse(result.stdout.trim() || '[]');

let md = `# Memory Review Wave Collection\n\n`;
md += `- parent_task_id: ${parentId}\n`;
md += `- generated_at: ${new Date().toISOString()}\n`;
md += `- task_count: ${rows.length}\n`;
md += `- max_field_chars: ${maxFieldChars}\n`;
md += `- max_body_chars: ${maxBodyChars}\n`;
md += `- note: secrets are best-effort redacted; verify before sharing outside local machine.\n\n`;

for (const row of rows) {
  md += `## ${row.id}\n\n`;
  md += `- status: ${row.status}\n`;
  md += `- assignee: ${row.assignee ?? ''}\n`;
  md += `- title: ${redactSecrets(row.title ?? '')}\n`;
  md += `- summary_at: ${row.summary_at ?? ''}\n\n`;
  if (row.conversation_id) {
    md += `- conversation_id: ${row.conversation_id}\n`;
    md += `- conversation_status: ${row.conversation_status ?? ''}\n`;
    md += `- conversation_ended_at: ${row.conversation_ended_at ?? ''}\n`;
    md += `- conversation_final_at: ${row.conversation_final_at ?? ''}\n\n`;
  }
  const bodyCandidates = [row.summary, row.latest_note, row.conversation_final].filter(Boolean);
  const body = bodyCandidates.sort((a, b) => String(b).length - String(a).length)[0] || '';
  const redacted = redactSecrets(body || 'No summary/note captured yet.');
  md += redacted.length > maxBodyChars ? `${redacted.slice(0, maxBodyChars)}\n\n[TRUNCATED]` : redacted;
  md += '\n\n---\n\n';
}

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, md);
console.log(JSON.stringify({ ok: true, out: path.relative(repoRoot, outPath), task_count: rows.length }, null, 2));
