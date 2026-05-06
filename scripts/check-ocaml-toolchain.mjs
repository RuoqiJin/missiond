#!/usr/bin/env node

import { toolchainStatus } from './lib/ocaml_lispc.mjs';

const usage = `Usage:
  node scripts/check-ocaml-toolchain.mjs [--json]

Checks whether the local OCaml checker toolchain is installed. This script
never installs anything; it reports missing commands and exits non-zero.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const status = toolchainStatus();
  const result = {
    ok: status.ok,
    toolchain: status,
    diagnostics: status.ok
      ? []
      : [{
          file: 'tools/missiond_lispc',
          line: 1,
          column: 1,
          code: 'OCAML_TOOLCHAIN_MISSING',
          message: `missing OCaml command(s): ${status.missing.join(', ')}. ${status.install_hint}`,
          path: '',
        }],
  };
  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('OCaml toolchain OK');
  } else {
    for (const d of result.diagnostics) console.error(`${d.code}: ${d.message}`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false };
  for (const arg of argv) {
    if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else {
      console.error(`unknown argument: ${arg}\n\n${usage}`);
      process.exit(2);
    }
  }
  return opts;
}

main();
