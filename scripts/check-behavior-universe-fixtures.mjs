#!/usr/bin/env node

import { runBehaviorUniverseFixtures } from './lib/behavior_universe_fixtures.mjs';

const usage = `Usage:
  node scripts/check-behavior-universe-fixtures.mjs [--json]

Runs self-contained scanner/checker fixtures for behavior-universe closure.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const result = runBehaviorUniverseFixtures();
  if (opts.json) {
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } else if (result.ok) {
    console.log(`behavior-universe fixtures OK (${result.cases.length} cases)`);
  } else {
    for (const d of result.diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.code}: ${d.message}`);
    }
    console.error(`behavior-universe fixtures FAILED -- ${result.diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false };
  for (const arg of argv) {
    if (arg === '--json') opts.json = true;
    else if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else {
      console.error(`error: unknown argument ${arg}\n\n${usage}`);
      process.exit(2);
    }
  }
  return opts;
}

main();
