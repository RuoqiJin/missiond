#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const DEFAULT_DB = process.env.MISSION_PG_DATABASE || 'missiond';
const DEFAULT_OUT = `.missiond/research/user-utterances-${new Date()
  .toISOString()
  .slice(0, 10)
  .replaceAll('-', '')}.md`;

function parseArgs(argv) {
  const opts = {
    db: DEFAULT_DB,
    out: DEFAULT_OUT,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--db') opts.db = argv[++i];
    else if (arg === '--out') opts.out = argv[++i];
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/export-human-user-utterances.mjs [--db missiond] [--out path.md]');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function psql(db, sql) {
  const result = spawnSync('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1', '-At', '-c', sql], {
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 200,
  });
  if (result.status !== 0) {
    throw new Error(`psql failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
  return result.stdout;
}

function loadCandidates(db) {
  const sql = `SELECT COALESCE(json_agg(t ORDER BY t.timestamp NULLS LAST, t.message_id), '[]'::json)
FROM (
  SELECT
    c.source,
    c.conversation_type,
    c.chat_type,
    c.id AS session_id,
    c.project,
    c.slot_id,
    c.task_id,
    m.id AS message_id,
    m.timestamp,
    m.role,
    m.raw_role,
    m.content
  FROM conversations c
  JOIN conversation_messages m ON m.session_id = c.id
  WHERE c.source IN ('claude_code', 'codex_cli', 'gemini_cli')
    AND m.role = 'user'
    AND COALESCE(m.raw_role, 'user') = 'user'
    AND NOT EXISTS (
      SELECT 1
      FROM message_labels ml
      WHERE ml.message_id = m.id
        AND ml.label = 'canonical_state'
        AND ml.value = 'equivalent-duplicate'
    )
    AND NOT EXISTS (
      SELECT 1
      FROM message_labels ml
      WHERE ml.message_id = m.id
        AND ml.source = 'claudecode-origin-labeler'
        AND (
          (ml.label = 'speaker' AND ml.value IN ('missiond_runtime', 'provider_system', 'terminal_artifact', 'worker_agent', 'subagent'))
          OR (ml.label = 'origin_layer' AND ml.value IN ('local_command', 'missiond_prompt', 'provider_context'))
        )
    )
    AND (
      (
        c.source = 'claude_code'
        AND c.conversation_type IN ('user', 'chat')
        AND COALESCE(c.chat_type, '') <> 'pty'
        AND COALESCE(c.task_id, '') = ''
        AND COALESCE(c.slot_id, '') = ''
      )
      OR (
        c.source = 'claude_code'
        AND c.conversation_type = 'history_prompt'
        AND c.chat_type = 'history_jsonl'
        AND COALESCE(c.task_id, '') = ''
        AND COALESCE(c.slot_id, '') = ''
      )
      OR (
        c.source = 'codex_cli'
        AND c.conversation_type = 'codex_chat'
        AND COALESCE(c.task_id, '') = ''
        AND COALESCE(c.slot_id, '') = ''
      )
      OR (
        c.source = 'gemini_cli'
        AND c.conversation_type = 'gemini_chat'
        AND COALESCE(c.task_id, '') = ''
        AND COALESCE(c.slot_id, '') = ''
        AND COALESCE(c.chat_type, '') IN ('gemini_cli', 'pty')
      )
    )
) t`;
  return JSON.parse(psql(db, sql).trim() || '[]');
}

const AUTOMATED_PATTERNS = [
  /^MissionD resident master tick\./,
  /^Execute MissionD task\b/,
  /^Read-only smoke\b/i,
  /^Read-only MissionD\b/i,
  /^POOL_SMOKE_/,
  /^# Codex Conversation Memory Candidate Sample\b/,
  /^Smoke test post-/i,
  /^Implement accepted swarm shard\b/i,
  /^<local-command-/,
  /^<command-name>/,
  /^\[Request interrupted/,
  /^\(Bash completed/i,
  /^\[Matched Skills[^\n]*\n[\s\S]*\bREAD-ONLY\b/i,
];

function automatedReason(content) {
  const text = content.trim();
  for (const pattern of AUTOMATED_PATTERNS) {
    if (pattern.test(text)) return `pattern:${pattern.source}`;
  }
  if (text.includes('## Swarm metadata') || text.includes('## Completion protocol')) {
    return 'swarm-or-worker-prompt';
  }
  if (text.includes('**Board Task ID**') || text.includes('BoardTask ID')) {
    return 'boardtask-worker-prompt';
  }
  if (text.includes('When done, write the task report') && text.includes('Task contract SSOT')) {
    return 'missiond-worker-brief';
  }
  return null;
}

function extractCodexRequest(content) {
  const marker = '## My request for Codex:';
  const idx = content.indexOf(marker);
  if (idx >= 0) {
    return {
      text: content.slice(idx + marker.length).trim(),
      note: 'codex_ide_context_stripped_to_my_request',
    };
  }
  if (content.startsWith('# Context from my IDE setup:')) {
    return {
      text: '',
      note: 'dropped_codex_ide_context_without_request_marker',
      reject: true,
    };
  }
  return { text: content.trim(), note: 'verbatim' };
}

function normalize(row) {
  const original = row.content ?? '';
  if (!original.trim()) return { reject: true, reason: 'empty' };

  const auto = automatedReason(original);
  if (auto) return { reject: true, reason: auto };

  let text = original.trim();
  let note = 'verbatim';
  if (row.source === 'codex_cli') {
    const extracted = extractCodexRequest(text);
    if (extracted.reject) return { reject: true, reason: extracted.note };
    text = extracted.text;
    note = extracted.note;
  }

  const autoAfterExtract = automatedReason(text);
  if (autoAfterExtract) return { reject: true, reason: autoAfterExtract };
  if (!text.trim()) return { reject: true, reason: 'empty_after_normalization' };

  return { reject: false, text, note };
}

function mdEscapeFence(text) {
  return text.replaceAll('```', '``\\`');
}

function summarize(records, rejected) {
  const by = (items, keyFn) => {
    const out = new Map();
    for (const item of items) {
      const key = keyFn(item);
      out.set(key, (out.get(key) ?? 0) + 1);
    }
    return [...out.entries()].sort((a, b) => String(a[0]).localeCompare(String(b[0])));
  };
  return {
    included_total: records.length,
    rejected_total: rejected.length,
    included_by_source: Object.fromEntries(by(records, (r) => r.source)),
    rejected_by_reason: Object.fromEntries(by(rejected, (r) => r.reason)),
    included_by_source_and_type: Object.fromEntries(
      by(records, (r) => `${r.source}/${r.conversation_type}/${r.chat_type ?? ''}`),
    ),
  };
}

function verify(records) {
  const suspiciousPatterns = [
    ['worker_or_boardtask', /BoardTask ID|Task contract SSOT|Execute MissionD task|## Swarm metadata|## Completion protocol/],
    ['resident_master_tick', /^MissionD resident master tick\./],
    ['local_command_artifact', /^<local-command-|^<command-name>|^\[Request interrupted|^\(Bash completed/i],
    ['codex_ide_context_preamble', /^# Context from my IDE setup:/],
  ];
  const hits = [];
  for (const record of records) {
    for (const [name, re] of suspiciousPatterns) {
      if (re.test(record.text)) {
        hits.push({
          name,
          source: record.source,
          session_id: record.session_id,
          message_id: record.message_id,
        });
      }
    }
  }
  const structural = records.filter(
    (r) =>
      ['worker', 'subagent', 'compaction'].includes(r.conversation_type) ||
      (r.task_id && String(r.task_id).trim()) ||
      (r.role !== 'user' || (r.raw_role ?? 'user') !== 'user'),
  );
  return {
    ok: hits.length === 0 && structural.length === 0,
    suspicious_pattern_hits: hits,
    structural_violations: structural.map((r) => ({
      source: r.source,
      conversation_type: r.conversation_type,
      session_id: r.session_id,
      message_id: r.message_id,
      task_id: r.task_id,
      role: r.role,
      raw_role: r.raw_role,
    })),
  };
}

function render(outPath, candidates, records, rejected, summary, verification) {
  const lines = [];
  lines.push('# MissionD Managed CLI True User Utterances Export');
  lines.push('');
  lines.push(`Generated at: ${new Date().toISOString()}`);
  lines.push(`Database: ${DEFAULT_DB}`);
  lines.push('');
  lines.push('## Scope');
  lines.push('');
  lines.push('- Sources: `claude_code`, `codex_cli`, `gemini_cli`.');
  lines.push('- Included only top-level human candidate messages: role `user`, raw role `user`.');
  lines.push('- ClaudeCode `~/.claude/history.jsonl` prompt-only rows are included as historical human prompts.');
  lines.push('- Excluded worker/subagent/compaction conversations, task-bound sessions, resident master ticks, smoke prompts, BoardTask/Swarm prompts, and local-command artifacts.');
  lines.push('- Codex IDE context messages are stripped to the text after `## My request for Codex:`.');
  lines.push('- ClaudeCode `pty` chat rows are excluded because they contain replayed terminal/log context; Gemini direct `pty` rows are included only when unbound to any task or slot.');
  lines.push('- ClaudeCode origin labels are honored: MissionD prompts, provider context, terminal artifacts, worker messages, and subagent messages are excluded.');
  lines.push('');
  lines.push('## Verification');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify({ candidates: candidates.length, summary, verification }, null, 2));
  lines.push('```');
  lines.push('');
  lines.push('## Utterances');
  lines.push('');
  records.forEach((r, idx) => {
    lines.push(`### ${String(idx + 1).padStart(6, '0')} · ${r.source} · message ${r.message_id}`);
    lines.push('');
    lines.push(`- timestamp: ${r.timestamp ?? ''}`);
    lines.push(`- session_id: ${r.session_id}`);
    lines.push(`- conversation_type: ${r.conversation_type}`);
    lines.push(`- chat_type: ${r.chat_type ?? ''}`);
    lines.push(`- project: ${r.project ?? ''}`);
    lines.push(`- normalization: ${r.normalization_note}`);
    lines.push('');
    lines.push('```text');
    lines.push(mdEscapeFence(r.text));
    lines.push('```');
    lines.push('');
  });
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, `${lines.join('\n')}\n`);
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const candidates = loadCandidates(opts.db);
  const records = [];
  const rejected = [];
  for (const row of candidates) {
    const normalized = normalize(row);
    if (normalized.reject) {
      rejected.push({ ...row, reason: normalized.reason });
      continue;
    }
    records.push({
      ...row,
      text: normalized.text,
      normalization_note: normalized.note,
    });
  }
  const summary = summarize(records, rejected);
  const verification = verify(records);
  render(opts.out, candidates, records, rejected, summary, verification);
  console.log(
    JSON.stringify(
      {
        ok: verification.ok,
        out: opts.out,
        candidates: candidates.length,
        included: records.length,
        rejected: rejected.length,
        summary,
        verification,
      },
      null,
      2,
    ),
  );
  if (!verification.ok) process.exitCode = 2;
}

main();
