#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';

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
  args.get('manifest') ?? '.missiond/research/memory-review/manifest.json',
);
const start = Number(args.get('start') ?? 1);
const count = Number(args.get('count') ?? 8);
const parentIdArg = args.get('parent-id') ?? args.get('parentId') ?? '';
const dryRun = args.get('dry-run') === 'true';
const delayMs = Number(args.get('delay-ms') ?? 8000);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
if (!Number.isInteger(start) || start < 1) throw new Error('--start must be >= 1');
if (!Number.isInteger(count) || count < 1) throw new Error('--count must be >= 1');
if (!Number.isFinite(delayMs) || delayMs < 0) throw new Error('--delay-ms must be >= 0');

function callTool(toolName, toolArgs) {
  return new Promise((resolve, reject) => {
    const bin = process.env.MISSION_MCP_BIN ?? `${process.env.HOME}/.xjp-mission/mission-mcp`;
    const child = spawn(bin, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        MISSIOND_MCP_PRELOAD_INSTRUCTIONS: '0',
        MISSION_LOG_LEVEL: process.env.MISSION_LOG_LEVEL ?? 'error',
      },
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString('utf8');
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString('utf8');
    });
    const timeout = setTimeout(() => {
      child.kill('SIGTERM');
      reject(new Error(`MCP call timed out: ${toolName}\n${stderr}`));
    }, Number(process.env.MISSION_MCP_CALL_TIMEOUT_MS ?? 120000));
    child.on('close', (code) => {
      clearTimeout(timeout);
      if (code !== 0) {
        reject(new Error(`MCP call failed (${code}): ${toolName}\n${stderr}\n${stdout}`));
        return;
      }
      const responses = stdout
        .trim()
        .split(/\n+/)
        .filter(Boolean)
        .map((line) => JSON.parse(line));
      const response = responses.find((item) => item.id === 2) ?? responses.at(-1);
      if (!response) {
        reject(new Error(`MCP call produced no response: ${toolName}\n${stderr}`));
        return;
      }
      resolve(response);
    });
    const requests = [
      {
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: {
          protocolVersion: '2024-11-05',
          capabilities: {},
          clientInfo: { name: 'dispatch-memory-review-wave', version: '0.1.0' },
        },
      },
      { jsonrpc: '2.0', method: 'notifications/initialized' },
      {
        jsonrpc: '2.0',
        id: 2,
        method: 'tools/call',
        params: { name: toolName, arguments: toolArgs },
      },
    ];
    for (const request of requests) {
      child.stdin.write(JSON.stringify(request));
      child.stdin.write('\n');
    }
    child.stdin.end();
  });
}

function extractToolText(response) {
  const content = response?.result?.content;
  if (!Array.isArray(content)) return '';
  return content.map((item) => item.text ?? '').join('\n');
}

function parseToolJsonText(response) {
  const text = extractToolText(response).trim();
  if (!text) return {};
  try {
    return JSON.parse(text);
  } catch {
    return { text };
  }
}

function batchObjective(batch) {
  return `请审视 MissionD 真实用户发言记忆候选 ${batch.id}。只读 ${batch.path} 和 ${manifest.worker_brief_path}，从 ${batch.item_count} 条原文里筛选值得进入 active memory 的少数候选。请严格保留约 10% 以下，优先找长期偏好、架构原则、项目常量、workflow 规则、工具能力边界、未解决基建债。不要改文件、不要写数据库、不要提交。最终报告必须使用 worker brief 里的精确 Markdown 标题：Findings / Active Memory Candidates / SSOT-Workflow Backfill Candidates / Needs Human / Discard Rationale / Verification。尽量给出原文摘录；遇到密钥/令牌必须脱敏。`;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
  const selected = manifest.batches.slice(start - 1, start - 1 + count);
  if (selected.length === 0) throw new Error('No batches selected');

  let parentId = parentIdArg;
  let parentResponse = null;
  if (!parentId && !dryRun) {
    parentResponse = await callTool('mission_board_create', {
      title: 'KB memory triage wave: true-user utterance review',
      description: [
        'Review true user utterances exported from MissionD-managed provider logs.',
        `Manifest: ${manifest.output_dir}/manifest.json`,
        `Worker brief: ${manifest.worker_brief_path}`,
        `Total utterances: ${manifest.total_utterances}`,
        'Workers are read-only and must only return candidate original text/excerpts plus rationale.',
      ].join('\n'),
      priority: 'medium',
      category: 'memory',
      project: 'missiond',
      auto_execute: false,
    });
    const parentPayload = parseToolJsonText(parentResponse);
    parentId = parentPayload.id ?? parentPayload.task_id ?? '';
  }

  const results = [];
  for (const batch of selected) {
    const taskArgs = {
      objective: batchObjective(batch),
      intent: 'code',
      cwd: repoRoot,
      priority: 'medium',
      timeout_secs: 7200,
      task_class: 'review',
      engine_hint: 'claude-code',
      pool_hint: 'claude-code-default',
      model_profile: 'coding-default-opus-4-7',
      context_pack_path: path.relative(repoRoot, path.resolve(repoRoot, manifest.output_dir, 'manifest.lisp')),
      read_scope: [
        path.resolve(repoRoot, batch.path),
        path.resolve(repoRoot, manifest.worker_brief_path),
        path.resolve(repoRoot, manifest.output_dir, 'manifest.json'),
      ],
      write_scope: [],
      must_not_touch: ['**/*'],
      acceptance: [
        'Read-only: do not edit, stage, commit, or write DB.',
        'Final report must contain exact Markdown headings: ## Findings / ## Active Memory Candidates / ## SSOT-Workflow Backfill Candidates / ## Needs Human / ## Discard Rationale / ## Verification.',
      ],
      parent_id: parentId || undefined,
      source_id: parentId || undefined,
    };
    if (dryRun) {
      results.push({ batch_id: batch.id, args: taskArgs });
      continue;
    }
    const response = await callTool('mission_task_delegate', taskArgs);
    const payload = parseToolJsonText(response);
    results.push({
      batch_id: batch.id,
      batch_path: batch.path,
      task_id: payload.task_id ?? null,
      assignee: payload.assignee ?? null,
      status: payload.status ?? null,
      raw: payload,
    });
    if (delayMs > 0 && batch !== selected.at(-1)) {
      await sleep(delayMs);
    }
  }

  const wave = {
    ok: true,
    dry_run: dryRun,
    parent_task_id: parentId || null,
    start,
    count: selected.length,
    manifest: path.relative(repoRoot, manifestPath),
    results,
  };
  const waveDir = path.resolve(repoRoot, manifest.output_dir, 'waves');
  fs.mkdirSync(waveDir, { recursive: true });
  const wavePath = path.join(
    waveDir,
    `wave-${String(start).padStart(4, '0')}-${String(start + selected.length - 1).padStart(4, '0')}.json`,
  );
  fs.writeFileSync(wavePath, JSON.stringify(wave, null, 2) + '\n');
  console.log(JSON.stringify({ ...wave, wave_path: path.relative(repoRoot, wavePath) }, null, 2));
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
