#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Read-only doctor alias for `install-missiond-hooks.mjs --check`.
//
// Keeps the CLI surface clean: agents and humans who only want to inspect
// state never need to remember the install/check flag combo. This script
// performs ZERO mutations — it always delegates with --check, regardless of
// what the user passes.
//
// Flags forwarded:
//   --json     machine-readable output
//   --strict   make drift a hard non-zero exit
//
// Mutating flags (--install, --dry-fixture, etc.) are rejected here so the
// doctor stays predictable. Use install-missiond-hooks.mjs directly when you
// need to mutate.

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const INSTALLER = path.join(SCRIPT_DIR, 'install-missiond-hooks.mjs');

const usage = `Usage:
  node scripts/check-missiond-hooks.mjs [--json] [--strict]

Read-only doctor for git core.hooksPath = .githooks. Delegates to
\`install-missiond-hooks.mjs --check\` and forwards exit code + output.

Flags:
  --json     machine-readable JSON output
  --strict   exit non-zero when core.hooksPath != .githooks or the hook
             file is missing

Use \`node scripts/install-missiond-hooks.mjs --install\` when you want to
mutate git config.
`;

function failUsage(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function main() {
  const argv = process.argv.slice(2);
  const forwarded = ['--check'];

  for (const arg of argv) {
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json' || arg === '--strict') {
      forwarded.push(arg);
    } else if (
      arg === '--install' ||
      arg === '--dry-fixture' ||
      arg === '--check'
    ) {
      failUsage(
        `flag ${arg} is rejected by the doctor alias; this command always runs --check. ` +
          `Run install-missiond-hooks.mjs directly if you need a different mode.`,
      );
    } else if (arg.startsWith('--')) {
      failUsage(`unknown flag: ${arg}`);
    } else {
      failUsage(`unexpected positional argument: ${arg}`);
    }
  }

  const child = spawnSync('node', [INSTALLER, ...forwarded], { stdio: 'inherit' });
  if (child.error) {
    process.stderr.write(
      `check-missiond-hooks: failed to invoke installer: ${child.error.message ?? child.error}\n`,
    );
    process.exit(1);
  }
  process.exit(child.status ?? 1);
}

main();
