#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const json = process.argv.includes('--json');
const repo = process.cwd();

const checks = [
  [
    '.missiond/v3/missiond-blueprint.lisp',
    [
      'codex-boot-context-policy',
      '(function codex-boot-context',
      'mission_context_boot',
      '.missiond/v3/evidence/codex-boot-context.lisp',
      'L0-always-on',
      'L1-current-task',
      'L2-grounded-facts',
      'L3-cold-evidence',
      'missiond-cli/xjp-cli',
    ],
  ],
  [
    '.missiond/v3/evidence/codex-boot-context.lisp',
    [
      '(codex-boot-context',
      ':schema "missiond.codex-boot-context.v1"',
      'Lisp is the SSOT',
      'mission_context_gather',
      'scripts/mission-context-pack.mjs',
      'Codex App',
      'accepted_shard_id',
      'task-result-artifact',
      'PTY is diagnostic only',
      'missiond-cli',
      'xjp-cli',
      'missiond-mcp',
      'xjp-mcp',
    ],
  ],
  [
    'AGENTS.md',
    [
      'Codex App Context Pack Bootstrap',
      'node scripts/mission-context-pack.mjs --json',
      'intent_candidates',
      'suggested_first_reads',
      'evidence_lanes',
      'avoid_first_reads',
    ],
  ],
  [
    'scripts/mission-context-pack.mjs',
    [
      'missiond.codex-app-context-pack.v1',
      'intent_candidates',
      'suggested_first_reads',
      'evidence_lanes',
      'avoid_first_reads',
      'mission_context_boot',
      'mission_context_gather',
      'raw_history_not_startup_context',
    ],
  ],
  [
    'crates/missiond-daemon/src/handlers/mod.rs',
    ['"mission_context_boot" | "mission_context_gather"', 'context_gather::handle'],
  ],
  [
    'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
    [
      'missiond.codex-boot-context.v1',
      'codex-boot-context.lisp',
      'handle_context_boot',
    ],
  ],
  [
    'crates/missiond-mcp/src/tools/knowledge/context_gather.rs',
    ['mission_context_boot', 'Codex Boot Context Capsule'],
  ],
  [
    'scripts/check-v3-code-isomorphism-complete.mjs',
    ['scripts/check-v3-codex-boot-context-isomorphism.mjs'],
  ],
];

const diagnostics = [];
for (const [rel, needles] of checks) {
  const file = path.join(repo, rel);
  if (!fs.existsSync(file)) {
    diagnostics.push({ file: rel, message: 'missing file' });
    continue;
  }
  const text = rel === '.missiond/v3/missiond-blueprint.lisp'
    ? readBlueprintWithEvidenceSidecars(repo, rel)
    : fs.readFileSync(file, 'utf8');
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({ file: rel, message: `missing ${needle}` });
    }
  }
}

const capsule = fs.readFileSync(path.join(repo, '.missiond/v3/evidence/codex-boot-context.lisp'), 'utf8');
const secretPatterns = [
  /BEGIN (RSA |OPENSSH |EC |)PRIVATE KEY/i,
  /AKIA[0-9A-Z]{16}/,
  /LTAI[0-9A-Za-z]{12,}/,
  /(api[_-]?key|access[_-]?key|secret|token)\s*[:=]\s*["']?[A-Za-z0-9_\-]{20,}/i,
];
for (const pattern of secretPatterns) {
  if (pattern.test(capsule)) {
    diagnostics.push({ file: '.missiond/v3/evidence/codex-boot-context.lisp', message: `possible secret pattern ${pattern}` });
  }
}

const ok = diagnostics.length === 0;
if (json) {
  console.log(JSON.stringify({ ok, diagnostics }, null, 2));
} else if (ok) {
  console.log('v3 codex boot context isomorphism check OK');
} else {
  console.error(`v3 codex boot context isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
}
process.exit(ok ? 0 : 1);
