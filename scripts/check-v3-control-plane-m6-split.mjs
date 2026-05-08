#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const json = process.argv.includes('--json');
const repo = process.cwd();

const checks = [
  [
    '.missiond/v3/missiond-blueprint.lisp',
    [
      '(control-plane-m6-split',
      ':schema "missiond.control-plane-m6-split.v1"',
      ':domains [workstation-control-plane master-control-plane eventbridge-deployment-plane project-universe-plane knowledge-skill-plane]',
      '(domain workstation-control-plane',
      ':functions [model-profile-policy slot-template-registry startup-slot-registry timeout-capacity-ttl-policy dispatch-routing-policy prompt-context-policy exact-shard-contract provider-interaction-policy]',
      '(domain master-control-plane',
      ':functions [master-checkpoint master-event-intake master-objective-loop master-delegation-loop master-recovery-loop master-maintenance-loop mcp-readiness]',
      '(domain eventbridge-deployment-plane',
      ':functions [event-envelope-contract event-waiter-contract deployment-event-ingest deployment-provenance-policy deploy-center-relay-contract deploy-agent-update-provenance]',
      '(domain project-universe-plane',
      ':functions [project-identity-root-resolution registry-authority-map maturity-contract service-runtime-summary deploy-fact-reference forge-catalog-reference registry-reconciliation]',
      '(domain knowledge-skill-plane',
      ':functions [skill-registry skill-search skill-project-links skill-to-workflow-promotion memory-quarantine memory-distillation memory-search-v2]',
      ':workflow ".missiond/workflows/missiond-control-plane-m6-split.lisp"',
      ':checker "node scripts/check-v3-control-plane-m6-split.mjs"',
      '"node scripts/check-v3-control-plane-m6-split.mjs"',
    ],
  ],
  [
    '.missiond/workflows/missiond-control-plane-m6-split.lisp',
    [
      '(workflow missiond-control-plane-m6-split',
      ':schema "missiond.workflow.v1"',
      ':status active',
      ':id review-question',
      ':id inventory-overloaded-blocks',
      ':id define-subplanes',
      ':id preserve-runtime-compatibility',
      ':id add-checker-pins',
      ':id compile-projection',
      ':id report-residual-gaps',
      'check-v3-control-plane-m6-split',
    ],
  ],
  [
    'scripts/check-v3-code-isomorphism-complete.mjs',
    ['scripts/check-v3-control-plane-m6-split.mjs'],
  ],
  [
    'scripts/check-v3-final-convergence.mjs',
    ['control-plane-m6-split', 'check-v3-control-plane-m6-split.mjs'],
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

const blueprint = path.join(repo, '.missiond/v3/missiond-blueprint.lisp');
if (fs.existsSync(blueprint)) {
  const text = fs.readFileSync(blueprint, 'utf8');
  const domainMatches = [...text.matchAll(/\(domain\s+([^\s\)]+)/g)].map((m) => m[1]);
  const required = [
    'workstation-control-plane',
    'master-control-plane',
    'eventbridge-deployment-plane',
    'project-universe-plane',
    'knowledge-skill-plane',
  ];
  for (const domain of required) {
    if (!domainMatches.includes(domain)) {
      diagnostics.push({ file: '.missiond/v3/missiond-blueprint.lisp', message: `missing domain ${domain}` });
    }
  }
}

const ok = diagnostics.length === 0;
if (json) {
  console.log(JSON.stringify({ ok, diagnostics }, null, 2));
} else if (ok) {
  console.log('v3 control-plane M6 split check OK');
} else {
  console.error(`v3 control-plane M6 split check FAILED -- ${diagnostics.length} diagnostic(s)`);
  for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
}
process.exit(ok ? 0 : 1);
