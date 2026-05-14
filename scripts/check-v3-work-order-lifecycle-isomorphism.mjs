#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const json = process.argv.includes('--json');
const repo = process.cwd();

const checks = [
  [
    '.missiond/v3/missiond-blueprint.lisp',
    [
      '(function work-order-lifecycle',
      '(surface work-order-lifecycle',
      'external-application-intent.lisp',
      'external-service-intent-envelope',
      'BoardTaskCreated',
      'mission_board_create',
      'plan.lisp',
      'audit.lisp',
      'task-result-artifacts',
      'workflow_run',
      'external translation, code-refactor, deploy-ops',
      'claude-code-deploy-ops',
      'cloud-ops-delegation-policy',
      'work-order-lifecycle is the single governance chain',
      'scripts/check-v3-work-order-lifecycle-isomorphism.mjs',
    ],
  ],
  [
    '.missiond/workflows/work-order-lifecycle.lisp',
    [
      '(workflow work-order-lifecycle',
      ':workflow_id work-order-lifecycle',
      ':status active',
      'BoardTaskCreated',
      'external service intent envelope',
      'translation',
      'code_refactor',
      'deploy_ops',
      'no-broad-implementation',
      'external-app-same-chain',
      'task-result-artifact',
      'audit.lisp',
    ],
  ],
  [
    '.missiond/work-orders/20260513-aliyun-global-accesskey-rotation/intent.lisp',
    [
      '(work-order-intent',
      ':schema "missiond.work-order.intent.v1"',
      'wo-20260513-aliyun-global-accesskey-rotation',
      'global Aliyun account credential',
      'External applications may submit the same work-order shape',
    ],
  ],
  [
    '.missiond/work-orders/20260513-aliyun-global-accesskey-rotation/plan.lisp',
    [
      '(work-order-plan',
      ':schema "missiond.work-order.plan.v1"',
      'secret-store://aliyun-global/ALIYUN_ACCESS_KEY_ID',
      'secret-store://aliyun-global/ALIYUN_ACCESS_KEY_SECRET',
      'dns-read-probe',
      'work-order-backfill',
    ],
  ],
  [
    '.missiond/work-orders/20260513-aliyun-global-accesskey-rotation/audit.lisp',
    [
      '(work-order-audit',
      ':schema "missiond.work-order.audit.v1"',
      'DescribeDomainRecords for changtu.pro succeeded',
      'No AccessKey secret value is stored',
    ],
  ],
  [
    'scripts/check-v3-code-isomorphism-complete.mjs',
    [
      "'work-order-lifecycle'",
      'scripts/check-v3-work-order-lifecycle-isomorphism.mjs',
    ],
  ],
];

const diagnostics = [];
for (const [rel, needles] of checks) {
  const file = path.join(repo, rel);
  if (!fs.existsSync(file)) {
    diagnostics.push({ file: rel, message: 'missing file' });
    continue;
  }
  const text = fs.readFileSync(file, 'utf8');
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({ file: rel, message: `missing ${needle}` });
    }
  }
}

const ok = diagnostics.length === 0;
if (json) {
  console.log(JSON.stringify({ ok, diagnostics }, null, 2));
} else if (ok) {
  console.log('v3 work-order lifecycle isomorphism check OK');
} else {
  console.error(`v3 work-order lifecycle isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
}
process.exit(ok ? 0 : 1);
