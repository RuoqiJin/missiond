#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { createRequire } from 'node:module';

const root = process.cwd();
const boardRequire = createRequire(path.join(root, 'packages/board/package.json'));
const ts = boardRequire('typescript');
const sourcePath = path.join(root, 'packages/board/src/lib/operatorOverview.ts');
const source = fs.readFileSync(sourcePath, 'utf8');

const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.CommonJS,
    target: ts.ScriptTarget.ES2020,
    esModuleInterop: true,
  },
  fileName: sourcePath,
}).outputText;

const module = { exports: {} };
vm.runInNewContext(compiled, { module, exports: module.exports, console }, { filename: sourcePath });

const { buildOperatorOverview, createFakeOperatorOverviewHarness } = module.exports;
if (typeof buildOperatorOverview !== 'function' || typeof createFakeOperatorOverviewHarness !== 'function') {
  throw new Error('operatorOverview exports are missing');
}

const callTool = createFakeOperatorOverviewHarness({
  mission_board_list: [
    { id: 'task-running', title: 'Run task', status: 'running' },
    { id: 'task-done', title: 'Done task', status: 'done' },
  ],
  mission_slots: [{ id: 'slot-a' }, { id: 'slot-b' }],
  mission_pty_status: {
    'slot-a': { recognition: { state: 'running', provider: 'codex' } },
    'slot-b': { __error: 'slot status timeout' },
  },
  mission_question_list: [{ id: 'q1', question: 'Need input' }],
  mission_health: {
    eventBus: { dispatchLag: 150, lagged: 1, dlq: { count: 1 } },
    evidence: {
      completed: 1,
      missing: 1,
      degraded: true,
      items: [{ taskId: 'task-done', complete: false, summary: 'missing artifact' }],
      gate: { status: 'blocked', missing: 1 },
    },
    operatorTrends: {
      points: [{ sampledAt: '2026-05-26T00:00:00Z', eventDispatchLag: 150 }],
      windows: { '24h': { eventDispatchLagMax: 150, evidenceMissingMax: 1 } },
    },
    startupPreflight: { checks: [] },
  },
  mission_worker: {
    workers: [
      {
        name: 'worker-a',
        health: {
          lifecycle: 'failed',
          stale: true,
          lastError: 'boom',
          heartbeatAgeSecs: 180,
        },
      },
    ],
  },
});

const overview = await buildOperatorOverview(callTool);
const diagnostics = [];
if (!overview.partial) diagnostics.push('overview must be partial when one slot status fixture is missing');
if (!overview.errors.some((err) => err.source === 'slotStatus' && err.slotId === 'slot-b')) {
  diagnostics.push('slotStatus failure must include slotId');
}
if (!overview.trends?.windows?.['24h']) diagnostics.push('operator trends must be projected');
if (!overview.runbook.some((item) => item.action?.id === 'dlq_list')) {
  diagnostics.push('DLQ runbook item must include executable action');
}
if (!overview.runbook.some((item) => item.action?.id?.startsWith('worker_resume:'))) {
  diagnostics.push('worker runbook item must include executable worker action');
}
if (!overview.runbook.some((item) => item.source === 'evidence' && item.action?.id === 'open_evidence')) {
  diagnostics.push('evidence runbook item must include evidence action');
}

if (diagnostics.length) {
  console.error(JSON.stringify({ ok: false, diagnostics }, null, 2));
  process.exit(1);
}
console.log(JSON.stringify({ ok: true }, null, 2));
