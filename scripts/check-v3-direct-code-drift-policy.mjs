#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
};

function main() {
  const enforceDiff = process.argv.includes('--enforce-diff');
  const diagnostics = [];
  const blueprint = read(FILES.blueprint, diagnostics);
  const aggregate = read(FILES.aggregate, diagnostics);
  if (blueprint) {
    requireAll(diagnostics, FILES.blueprint, blueprint, [
      '(lisp-code-drift-policy',
      ':normal-rule',
      ':emergency-waiver',
      ':backfill-task',
      'Backfill Lisp/checker for emergency code change',
      '(surface lisp-code-drift-policy',
      'scripts/check-v3-direct-code-drift-policy.mjs',
    ]);
  }
  if (aggregate) {
    requireAll(diagnostics, FILES.aggregate, aggregate, [
      'scripts/check-v3-direct-code-drift-policy.mjs',
    ]);
  }
  if (enforceDiff) checkDiff(diagnostics);
  if (diagnostics.length > 0) {
    diagnostics.forEach((d) => console.error(d));
    console.error(`v3 direct-code-drift policy check FAILED -- ${diagnostics.length} diagnostic(s)`);
    process.exit(1);
  }
  console.log(enforceDiff ? 'v3 direct-code-drift policy check OK (diff enforced)' : 'v3 direct-code-drift policy check OK');
}

function read(rel, diagnostics) {
  try {
    return fs.readFileSync(path.join(process.cwd(), rel), 'utf8');
  } catch (err) {
    diagnostics.push(`${rel}: cannot read: ${err.message}`);
    return '';
  }
}

function checkDiff(diagnostics) {
  const result = spawnSync('git', ['diff', '--name-only'], { encoding: 'utf8' });
  if (result.status !== 0) {
    diagnostics.push(`git diff --name-only failed: ${result.stderr || result.stdout}`);
    return;
  }
  const changed = result.stdout.split('\n').map((s) => s.trim()).filter(Boolean);
  const codeChanged = changed.some((rel) => /^(crates|packages|scripts)\//.test(rel));
  const lispChanged = changed.some((rel) => rel.endsWith('.lisp') || rel.includes('/evidence/'));
  if (codeChanged && !lispChanged) {
    diagnostics.push(
      'code diff has no Lisp/evidence delta; create emergency waiver/backfill BoardTask or update the corresponding blueprint surface',
    );
  }
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(`${file}: missing required text: ${needle}`);
  }
}

main();
