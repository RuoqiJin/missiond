#!/usr/bin/env node

import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { runLispc } from './lib/ocaml_lispc.mjs';

const DEFAULT_AUTH_MISSIOND =
  '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/.missiond';
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const MISSIOND_ROOT = path.resolve(SCRIPT_DIR, '..');

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const result = runLispc(['check-auth-domain', '--dir', opts.dir], { repoRoot: MISSIOND_ROOT });
  const output = {
    ok: result.ok === true,
    dir: opts.dir,
    diagnostics: result.diagnostics ?? [],
    engine: {
      mode: result.unavailable ? 'unavailable' : 'ocaml',
      toolchain: result.toolchain ?? null,
    },
  };
  if (opts.json) console.log(JSON.stringify(output, null, 2));
  else if (output.ok) console.log('auth domain SSOT OCaml check OK');
  else {
    for (const d of output.diagnostics) {
      console.error(`${d.file}:${d.line ?? 1}:${d.column ?? 1}: ${d.code ?? 'AUTH_DOMAIN'}: ${d.message}`);
    }
  }
  process.exit(output.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, dir: DEFAULT_AUTH_MISSIOND };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--dir') opts.dir = argv[++i] ?? fail('--dir requires a value');
    else if (arg.startsWith('--dir=')) opts.dir = arg.slice('--dir='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/check-auth-domain-ssot.mjs [--json] [--dir <auth .missiond dir>]');
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

main();
