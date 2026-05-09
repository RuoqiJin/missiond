#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

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

const inputPath = path.resolve(
  repoRoot,
  args.get('input') ?? '.missiond/research/true-user-utterances-20260509.md',
);
const outDir = path.resolve(
  repoRoot,
  args.get('out') ?? '.missiond/research/memory-review',
);
const maxItems = Number(args.get('max-items') ?? 300);
const maxChars = Number(args.get('max-chars') ?? 180_000);
if (!Number.isFinite(maxItems) || maxItems <= 0) throw new Error('--max-items must be positive');
if (!Number.isFinite(maxChars) || maxChars <= 10_000) throw new Error('--max-chars must be > 10000');

const source = fs.readFileSync(inputPath, 'utf8');
const start = source.indexOf('\n### 000001 ');
if (start < 0) throw new Error(`Cannot find utterance section in ${inputPath}`);
const utteranceSource = source.slice(start + 1);
const matches = [...utteranceSource.matchAll(/^### \d{6} · /gm)];
const utterances = [];
for (let i = 0; i < matches.length; i += 1) {
  const from = matches[i].index;
  const to = i + 1 < matches.length ? matches[i + 1].index : utteranceSource.length;
  const block = utteranceSource.slice(from, to).trimEnd();
  const header = block.split('\n', 1)[0];
  const ordinal = header.match(/^### (\d{6})/)?.[1] ?? String(i + 1).padStart(6, '0');
  const messageId = header.match(/message ([0-9a-fA-F-]+)/)?.[1] ?? '';
  const timestamp = block.match(/^- timestamp: (.+)$/m)?.[1] ?? '';
  const project = block.match(/^- project: (.*)$/m)?.[1] ?? '';
  const text = block.match(/```text\n([\s\S]*?)\n```/)?.[1] ?? '';
  utterances.push({
    ordinal,
    messageId,
    timestamp,
    project,
    chars: block.length,
    textChars: text.length,
    block,
  });
}

fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(path.join(outDir, 'batches'), { recursive: true });
fs.mkdirSync(path.join(outDir, 'worker-reports'), { recursive: true });

const brief = `# Memory Review Worker Brief

You are reviewing real user utterances exported from MissionD-managed ClaudeCode/Codex/Gemini conversation logs.

Goal: select only the original user statements that should become ACTIVE MissionD memory candidates. Be strict: target roughly 10% active retention. Most utterances should be discard/cold-evidence because SSOT Lisp, code, or project registries already cover them.

Read only your assigned batch file. Do not edit files, do not write to the database, do not stage, do not commit.

For every candidate worth keeping, quote the original user text or the smallest useful excerpt. If the text contains API keys, passwords, cookies, tokens, private keys, or other secrets, do not quote the secret value; write "[REDACTED_SECRET]" and mark the candidate as "secret-handling".

Return a structured report with EXACTLY these Markdown section headings. Do not number or rename the headings. If a section has no items, write "None".

## Findings
- Count reviewed.
- Count selected for active memory.
- Count uncertain / needs human.

## Active Memory Candidates
Each item must include source ordinal, message id, timestamp, category, original quote/excerpt, rationale, and confidence.

## SSOT-Workflow Backfill Candidates
Facts that should move to Lisp/workflow/project constants rather than active memory.

## Needs Human
Only genuinely ambiguous high-impact items.

## Discard Rationale
Common reasons items were rejected.

## Verification
Confirm you did not modify files and only read the assigned batch.

Retention categories:
- long-term-user-preference
- project-constant
- workflow-rule
- architecture-principle
- tool-capability-boundary
- unresolved-infrastructure-debt
- secret-handling
- cold-evidence-only
- discard
`;
fs.writeFileSync(path.join(outDir, 'worker-brief.md'), brief);

const batches = [];
let current = [];
let currentChars = 0;
function flush() {
  if (current.length === 0) return;
  const idx = batches.length + 1;
  const id = `memory-review-batch-${String(idx).padStart(4, '0')}`;
  const filename = `${id}.md`;
  const batchPath = path.join(outDir, 'batches', filename);
  const body = current.map((u) => u.block).join('\n\n');
  const hash = crypto.createHash('sha256').update(body).digest('hex');
  const first = current[0];
  const last = current[current.length - 1];
  const header = `# ${id}

- source_export: ${path.relative(repoRoot, inputPath)}
- batch_id: ${id}
- item_count: ${current.length}
- ordinal_range: ${first.ordinal}..${last.ordinal}
- timestamp_range: ${first.timestamp}..${last.timestamp}
- content_sha256: ${hash}
- worker_report_path: ${path.relative(repoRoot, path.join(outDir, 'worker-reports', `${id}.md`))}

Read this batch together with ../worker-brief.md.

`;
  fs.writeFileSync(batchPath, header + body + '\n');
  batches.push({
    id,
    path: path.relative(repoRoot, batchPath),
    report_path: path.relative(repoRoot, path.join(outDir, 'worker-reports', `${id}.md`)),
    item_count: current.length,
    ordinal_start: first.ordinal,
    ordinal_end: last.ordinal,
    timestamp_start: first.timestamp,
    timestamp_end: last.timestamp,
    content_sha256: hash,
    chars: body.length,
  });
  current = [];
  currentChars = 0;
}

for (const utterance of utterances) {
  const nextChars = currentChars + utterance.chars + 2;
  if (current.length > 0 && (current.length >= maxItems || nextChars > maxChars)) {
    flush();
  }
  current.push(utterance);
  currentChars += utterance.chars + 2;
}
flush();

const manifest = {
  schema: 'missiond.memory-review-batches.v1',
  generated_at: new Date().toISOString(),
  source_export: path.relative(repoRoot, inputPath),
  output_dir: path.relative(repoRoot, outDir),
  worker_brief_path: path.relative(repoRoot, path.join(outDir, 'worker-brief.md')),
  total_utterances: utterances.length,
  batch_count: batches.length,
  max_items: maxItems,
  max_chars: maxChars,
  batches,
};
fs.writeFileSync(path.join(outDir, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

let lisp = '(memory-review-manifest\n';
lisp += '  :schema "missiond.memory-review-batches.v1"\n';
lisp += `  :generated_at ${JSON.stringify(manifest.generated_at)}\n`;
lisp += `  :source_export ${JSON.stringify(manifest.source_export)}\n`;
lisp += `  :worker_brief_path ${JSON.stringify(manifest.worker_brief_path)}\n`;
lisp += `  :total_utterances ${manifest.total_utterances}\n`;
lisp += `  :batch_count ${manifest.batch_count}\n`;
lisp += '  :batches\n  (\n';
for (const batch of batches) {
  lisp += `    (batch :id ${JSON.stringify(batch.id)} :path ${JSON.stringify(batch.path)} :report_path ${JSON.stringify(batch.report_path)} :item_count ${batch.item_count} :ordinal_start "${batch.ordinal_start}" :ordinal_end "${batch.ordinal_end}" :sha256 "${batch.content_sha256}")\n`;
}
lisp += '  )\n)\n';
fs.writeFileSync(path.join(outDir, 'manifest.lisp'), lisp);

console.log(JSON.stringify({
  ok: true,
  total_utterances: utterances.length,
  batch_count: batches.length,
  manifest: path.relative(repoRoot, path.join(outDir, 'manifest.json')),
  worker_brief: manifest.worker_brief_path,
}, null, 2));
