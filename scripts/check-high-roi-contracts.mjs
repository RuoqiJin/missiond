#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-high-roi-contracts.mjs [--json]

Checks high-ROI MissionD repair contracts:
- source-state labels are centralized in missiond-core constants
- operator overview reports partial/errors diagnostics
- Board Event Health exposes stale-derived status
- mission_health exposes startupPreflight
`;

const SOURCE_STATE_LABELS = [
  'provider-index-missing',
  'missing-stale',
  'path-mismatch',
  'codex_local_index',
  'sqlite-missing',
];

const SOURCE_LABEL_ALLOW = new Set([
  'crates/missiond-core/src/types/conversation.rs',
]);

function main() {
  const args = process.argv.slice(2);
  const json = args.includes('--json');
  if (args.some((arg) => arg !== '--json' && arg !== '--help' && arg !== '-h')) {
    console.error(usage);
    process.exit(2);
  }
  if (args.includes('--help') || args.includes('-h')) {
    console.log(usage);
    process.exit(0);
  }

  const root = process.cwd();
  const diagnostics = [
    ...checkSourceStateConstants(root),
    ...checkRequiredText(root, 'packages/board/src/app/api/operator/overview/route.ts', [
      ['partial', /partial:\s*errors\.length\s*>\s*0/],
      ['errors array', /errors:\s*OverviewError\[\]/],
      ['error source', /source:\s*['"]slotStatus['"]/],
    ]),
    ...checkRequiredText(root, 'packages/board/src/eventStream.ts', [
      ['stale threshold', /EVENT_HEALTH_STALE_AFTER_MS/],
      ['stale flag', /eventHealthIsStale/],
      ['derived status', /eventHealthStatus/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/handlers/sysinfra/misc.rs', [
      ['startupPreflight health field', /startupPreflight/],
    ]),
  ];

  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('MissionD high-ROI contract check OK');
  } else {
    for (const diagnostic of diagnostics) {
      console.error(`${diagnostic.file}:${diagnostic.line}: ${diagnostic.message}`);
    }
    console.error(`MissionD high-ROI contract check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function checkSourceStateConstants(root) {
  const diagnostics = [];
  for (const rel of walk(path.join(root, 'crates'), root)) {
    if (!rel.endsWith('.rs')) continue;
    const source = fs.readFileSync(path.join(root, rel), 'utf8');
    const lines = source.split(/\r?\n/);
    lines.forEach((line, index) => {
      for (const label of SOURCE_STATE_LABELS) {
        const quoted = new RegExp(String.raw`["']${escapeRegExp(label)}["']`);
        if (quoted.test(line) && !SOURCE_LABEL_ALLOW.has(rel)) {
          diagnostics.push({
            file: rel,
            line: index + 1,
            message: `source-state label "${label}" must be referenced through missiond-core constants`,
          });
        }
      }
    });
  }
  return diagnostics;
}

function checkRequiredText(root, rel, expectations) {
  const abs = path.join(root, rel);
  if (!fs.existsSync(abs)) {
    return [{ file: rel, line: 1, message: 'required contract file is missing' }];
  }
  const source = fs.readFileSync(abs, 'utf8');
  return expectations
    .filter(([, pattern]) => !pattern.test(source))
    .map(([name]) => ({ file: rel, line: 1, message: `missing ${name} contract` }));
}

function walk(dir, root) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'target') continue;
    const abs = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...walk(abs, root));
    } else if (entry.isFile()) {
      out.push(path.relative(root, abs).split(path.sep).join(path.posix.sep));
    }
  }
  return out;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
