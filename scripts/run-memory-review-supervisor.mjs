#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
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

const manifestPath = path.resolve(
  repoRoot,
  args.get('manifest') ?? '.missiond/research/memory-review-v2/manifest.json',
);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const start = Number(args.get('start') ?? 1);
const end = Number(args.get('end') ?? manifest.batch_count);
const maxInflight = Number(args.get('max-inflight') ?? 16);
const burst = Number(args.get('burst') ?? 4);
const pollSecs = Number(args.get('poll-secs') ?? 45);
const settleSecs = Number(args.get('settle-secs') ?? 90);
const minFreeGb = Number(args.get('min-free-gb') ?? 20);
const autoCloseSettled = args.get('auto-close-settled') !== 'false';
const parentId = args.get('parent-id') ?? args.get('parentId');
const collectOut = path.resolve(
  repoRoot,
  args.get('collect-out') ?? `${manifest.output_dir}/collected-${parentId?.slice(0, 8) ?? 'parent'}-final.md`,
);
if (!parentId) throw new Error('--parent-id is required');
if (!Number.isInteger(start) || start < 1) throw new Error('--start must be >= 1');
if (!Number.isInteger(end) || end < start) throw new Error('--end must be >= start');
if (!Number.isInteger(maxInflight) || maxInflight < 1) throw new Error('--max-inflight must be >= 1');
if (!Number.isInteger(burst) || burst < 1) throw new Error('--burst must be >= 1');
if (!Number.isFinite(pollSecs) || pollSecs < 5) throw new Error('--poll-secs must be >= 5');
if (!Number.isFinite(settleSecs) || settleSecs < 30) throw new Error('--settle-secs must be >= 30');
if (!Number.isFinite(minFreeGb) || minFreeGb < 1) throw new Error('--min-free-gb must be >= 1');

const statePath = path.resolve(
  repoRoot,
  args.get('state') ?? `${manifest.output_dir}/supervisor-${parentId.slice(0, 8)}-${start}-${end}.json`,
);
fs.mkdirSync(path.dirname(statePath), { recursive: true });

function loadState() {
  if (!fs.existsSync(statePath)) {
    return {
      schema: 'missiond.memory-review-batch-runner.v1',
      parent_task_id: parentId,
      manifest: path.relative(repoRoot, manifestPath),
      start,
      end,
      next: start,
      waves: [],
      terminated_slots: [],
      auto_closed_tasks: [],
      task_result_artifacts: [],
      started_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };
  }
  return JSON.parse(fs.readFileSync(statePath, 'utf8'));
}

function saveState(state) {
  state.updated_at = new Date().toISOString();
  fs.writeFileSync(statePath, JSON.stringify(state, null, 2) + '\n');
}

function psqlJson(query) {
  const result = spawnSync('psql', ['-d', 'missiond', '-t', '-A', '-c', query], {
    encoding: 'utf8',
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || `psql failed ${result.status}`);
  }
  return JSON.parse(result.stdout.trim() || '[]');
}

function sha256(text) {
  return crypto.createHash('sha256').update(String(text)).digest('hex');
}

function freeDiskGb() {
  const result = spawnSync('df', ['-Pk', repoRoot], { encoding: 'utf8' });
  if (result.status !== 0) return null;
  const line = result.stdout.trim().split('\n').at(-1);
  const parts = line?.trim().split(/\s+/) ?? [];
  const availableKb = Number(parts[3]);
  return Number.isFinite(availableKb) ? availableKb / 1024 / 1024 : null;
}

function ensureDiskBudget(state) {
  const free = freeDiskGb();
  state.last_free_disk_gb = free == null ? null : Number(free.toFixed(2));
  if (free != null && free < minFreeGb) {
    state.paused_reason = `free disk ${free.toFixed(2)}GB is below --min-free-gb ${minFreeGb}GB`;
    state.paused_at = new Date().toISOString();
    saveState(state);
    console.log(JSON.stringify({ event: 'paused-low-disk', free_gb: state.last_free_disk_gb, min_free_gb: minFreeGb }));
    return false;
  }
  return true;
}

function sqlString(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

function taskStatusCounts() {
  return psqlJson(`
    select coalesce(json_agg(row_to_json(q)), '[]'::json)
    from (
      select status, count(*)::int as count
      from board_tasks
      where parent_id = ${sqlString(parentId)}
      group by status
      order by status
    ) q;
  `);
}

function nonTerminalCount() {
  return psqlJson(`
    select coalesce(json_agg(row_to_json(q)), '[]'::json)
    from (
      select count(*)::int as count
      from board_tasks
      where parent_id = ${sqlString(parentId)}
        and status not in ('done', 'failed', 'blocked', 'skipped')
    ) q;
  `)[0]?.count ?? 0;
}

function runDirectWave(next, count) {
  const cmd = [
    'scripts/dispatch-memory-review-direct-wave.mjs',
    '--manifest',
    path.relative(repoRoot, manifestPath),
    '--parent-id',
    parentId,
    '--start',
    String(next),
    '--count',
    String(count),
    '--delay-ms',
    '1500',
  ];
  const result = spawnSync('node', cmd, {
    cwd: repoRoot,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 16,
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || `dispatch failed ${result.status}`);
  }
  return JSON.parse(result.stdout.trim());
}

function collectWaveOutput() {
  const result = spawnSync(
    'node',
    [
      'scripts/collect-memory-review-wave.mjs',
      '--parent-id',
      parentId,
      '--out',
      path.relative(repoRoot, collectOut),
    ],
    {
      cwd: repoRoot,
      encoding: 'utf8',
      maxBuffer: 1024 * 1024 * 16,
    },
  );
  return {
    ok: result.status === 0,
    out: path.relative(repoRoot, collectOut),
    stdout: result.stdout?.trim() ?? '',
    stderr: result.stderr?.trim() ?? '',
    exit_code: result.status,
  };
}

function completedDynamicSlots() {
  return psqlJson(`
    select coalesce(json_agg(row_to_json(q)), '[]'::json)
    from (
      select distinct assignee as slot_id
      from board_tasks
      where parent_id = ${sqlString(parentId)}
        and status = 'done'
        and assignee like 'slot-dyn-%'
      order by assignee
    ) q;
  `);
}

function terminateSlot(slotId) {
  const result = spawnSync(
    'node',
    [
      'scripts/mission-mcp-call.mjs',
      'mission_compute_slot',
      JSON.stringify({ action: 'terminate', slot_id: slotId }),
    ],
    {
      cwd: repoRoot,
      encoding: 'utf8',
      maxBuffer: 1024 * 1024 * 4,
    },
  );
  return {
    ok: result.status === 0,
    slot_id: slotId,
    stdout: result.stdout?.trim() ?? '',
    stderr: result.stderr?.trim() ?? '',
    exit_code: result.status,
  };
}

function callMissionTool(toolName, toolArgs) {
  const result = spawnSync(
    'node',
    ['scripts/mission-mcp-call.mjs', toolName, JSON.stringify(toolArgs)],
    {
      cwd: repoRoot,
      encoding: 'utf8',
      maxBuffer: 1024 * 1024 * 8,
    },
  );
  return {
    ok: result.status === 0,
    stdout: result.stdout?.trim() ?? '',
    stderr: result.stderr?.trim() ?? '',
    exit_code: result.status,
  };
}

function parseMissionToolText(result) {
  if (!result?.stdout) return null;
  try {
    const rpc = JSON.parse(result.stdout);
    const text = rpc?.result?.content?.find?.((item) => item?.type === 'text')?.text;
    return text ? JSON.parse(text) : rpc;
  } catch {
    return null;
  }
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

function settledWorkerFinals() {
  return psqlJson(`
    select coalesce(json_agg(row_to_json(q)), '[]'::json)
    from (
      select
        t.id as task_id,
        t.title,
        t.assignee,
        c.id as conversation_id,
        msg.timestamp as final_at,
        msg.content as final_content
      from board_tasks t
      join lateral (
        select c.*
        from conversations c
        where c.task_id = t.id
        order by c.updated_at desc nulls last, c.started_at desc nulls last
        limit 1
      ) c on true
      join lateral (
        select cm.content, cm.timestamp
        from conversation_messages cm
        where cm.session_id = c.id
          and cm.role in ('assistant', 'agent_assistant')
          and coalesce(cm.content, '') like '%## Findings%'
          and coalesce(cm.content, '') like '%## Active Memory Candidates%'
          and coalesce(cm.content, '') like '%## Verification%'
        order by cm.timestamp desc nulls last, cm.id desc
        limit 1
      ) msg on true
      where t.parent_id = ${sqlString(parentId)}
        and t.status not in ('done', 'failed', 'blocked', 'skipped')
        and msg.timestamp < now() - (${Number(settleSecs)} * interval '1 second')
      order by msg.timestamp
    ) q;
  `);
}

function putTaskResultArtifact(row) {
  const dir = path.resolve(repoRoot, manifest.output_dir, 'task-result-artifacts');
  fs.mkdirSync(dir, { recursive: true });
  const content = redactSecrets(String(row.final_content ?? '').slice(0, 20000));
  const artifact = {
    schema: 'missiond.task-result-artifact.v1',
    task_id: row.task_id,
    title: row.title,
    slot_id: row.assignee,
    conversation_id: row.conversation_id,
    source: 'provider-durable-final',
    final_at: row.final_at,
    collected_at: new Date().toISOString(),
    content_sha256: sha256(content),
    content,
  };
  const out = path.join(dir, `${row.task_id}.json`);
  fs.writeFileSync(out, JSON.stringify(artifact, null, 2) + '\n');
  return path.relative(repoRoot, out);
}

function autoCloseSettledWorkerFinals(state) {
  if (!autoCloseSettled) return;
  const already = new Set(state.auto_closed_tasks ?? []);
  const rows = settledWorkerFinals().filter((row) => !already.has(row.task_id));
  const closed = [];
  for (const row of rows) {
    const content = [
      `Closed by memory-review supervisor after durable worker final settled for ${settleSecs}s.`,
      `conversation_id: ${row.conversation_id}`,
      `final_at: ${row.final_at}`,
      '',
      redactSecrets(String(row.final_content ?? '').slice(0, 12000)),
    ].join('\n');
    const settle = callMissionTool('mission_shared_memory', {
      action: 'worker_settle',
      task_id: row.task_id,
      project_id: 'missiond',
      slot_id: row.assignee,
      conversation_id: row.conversation_id,
      provider: 'claude_code',
      status: 'done',
      summary: `Memory review worker final settled for ${settleSecs}s.`,
      content,
    });
    const parsed = parseMissionToolText(settle);
    const fallbackArtifactPath = settle.ok ? null : putTaskResultArtifact(row);
    const ok = settle.ok && parsed?.ok !== false;
    closed.push({
      task_id: row.task_id,
      assignee: row.assignee,
      ok,
      closed_at: new Date().toISOString(),
      settle_error: settle.stderr.slice(-500),
      artifact_hash: parsed?.artifact_hash ?? null,
      task_result_artifact: fallbackArtifactPath,
    });
    if (ok) already.add(row.task_id);
  }
  state.auto_closed_tasks = Array.from(already).sort();
  state.task_result_artifacts = Array.from(
    new Set([
      ...(state.task_result_artifacts ?? []),
      ...closed
        .map((item) => item.artifact_hash ?? item.task_result_artifact)
        .filter(Boolean),
    ]),
  ).sort();
  if (closed.length > 0) {
    state.auto_close_events = [...(state.auto_close_events ?? []), ...closed].slice(-500);
    saveState(state);
    console.log(JSON.stringify({ event: 'auto-closed-settled-worker-finals', closed }));
  }
}

function reapCompletedDynamicSlots(state) {
  const already = new Set(state.terminated_slots ?? []);
  const rows = completedDynamicSlots();
  const reaped = [];
  for (const row of rows) {
    const slotId = row.slot_id;
    if (!slotId || already.has(slotId)) continue;
    const result = terminateSlot(slotId);
    reaped.push({
      slot_id: slotId,
      ok: result.ok,
      reaped_at: new Date().toISOString(),
      stderr_tail: result.stderr.slice(-500),
    });
    if (result.ok) {
      already.add(slotId);
    }
  }
  state.terminated_slots = Array.from(already).sort();
  if (reaped.length > 0) {
    state.reap_events = [...(state.reap_events ?? []), ...reaped].slice(-500);
    saveState(state);
    console.log(JSON.stringify({ event: 'reaped-completed-dynamic-slots', reaped }));
  }
}

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

let state = loadState();
saveState(state);
console.log(JSON.stringify({ event: 'batch-runner-start', state_path: path.relative(repoRoot, statePath), parentId, start, end, maxInflight, burst, pollSecs, settleSecs, minFreeGb, autoCloseSettled, collectOut: path.relative(repoRoot, collectOut) }));

while (true) {
  state = loadState();
  if (!ensureDiskBudget(state)) break;
  autoCloseSettledWorkerFinals(state);
  reapCompletedDynamicSlots(state);
  const inflight = nonTerminalCount();
  const counts = taskStatusCounts();
  console.log(JSON.stringify({ event: 'tick', next: state.next, inflight, counts, at: new Date().toISOString() }));

  const remaining = end - state.next + 1;
  const capacity = Math.max(0, maxInflight - inflight);
  const count = Math.min(remaining, capacity, burst);
  if (count > 0) {
    const wave = runDirectWave(state.next, count);
    state.waves.push({
      start: state.next,
      count,
      wave_path: wave.wave_path,
      task_ids: wave.results.map((item) => item.task_id),
      dispatched_at: new Date().toISOString(),
    });
    state.next += count;
    saveState(state);
    console.log(JSON.stringify({ event: 'dispatched', start: state.next - count, count, wave_path: wave.wave_path }));
  }

  if (state.next > end && nonTerminalCount() === 0) {
    const collection = collectWaveOutput();
    state.collection = collection;
    state.completed_at = new Date().toISOString();
    saveState(state);
    console.log(JSON.stringify({ event: 'supervisor-complete', state_path: path.relative(repoRoot, statePath), collection }));
    break;
  }

  sleep(pollSecs * 1000);
}
