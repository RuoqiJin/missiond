#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..');

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const mappedLevel = mapHardeningToMaturity(opts.minLevel);
  const argv = [
    path.join(SCRIPT_DIR, 'check-project-maturity.mjs'),
    '--min-level',
    mappedLevel,
    ...opts.projectIds.flatMap((id) => ['--project', id]),
    '--engine',
    opts.engine,
  ];
  if (opts.json) argv.push('--json');
  const proc = spawnSync(process.execPath, argv, {
    cwd: REPO_ROOT,
    encoding: 'utf8',
  });
  if (opts.json) {
    const maturity = parseJson(proc.stdout);
    process.stdout.write(`${JSON.stringify({
      ok: proc.status === 0 && maturity?.ok !== false,
      deprecated: true,
      replacement: 'node scripts/check-project-maturity.mjs --min-level M6',
      requested_h_level: opts.minLevel,
      mapped_m_level: mappedLevel,
      maturity,
      stderr_tail: tail(proc.stderr),
    }, null, 2)}\n`);
  } else {
    process.stderr.write('warning: check-project-domain-hardening.mjs is deprecated; use check-project-maturity.mjs --min-level M6.\n');
    process.stdout.write(proc.stdout ?? '');
    process.stderr.write(proc.stderr ?? '');
  }
  process.exit(proc.status ?? 1);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    summary: false,
    minLevel: 'H5',
    projectIds: [],
    engine: 'ocaml',
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--summary') opts.summary = true;
    else if (arg === '--min-level') opts.minLevel = argv[++i] ?? fail('--min-level requires a value');
    else if (arg.startsWith('--min-level=')) opts.minLevel = arg.slice('--min-level='.length);
    else if (arg === '--project') opts.projectIds.push(argv[++i] ?? fail('--project requires a value'));
    else if (arg.startsWith('--project=')) opts.projectIds.push(arg.slice('--project='.length));
    else if (arg === '--engine') opts.engine = argv[++i] ?? fail('--engine requires a value');
    else if (arg.startsWith('--engine=')) opts.engine = arg.slice('--engine='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/check-project-domain-hardening.mjs [--json] [--summary] [--min-level H0..H5] [--project <id>]');
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!/^H[0-5]$/.test(opts.minLevel)) fail(`unknown hardening level: ${opts.minLevel}`);
  if (!['auto', 'js', 'ocaml'].includes(opts.engine)) fail(`unknown engine: ${opts.engine}`);
  return opts;
}

function mapHardeningToMaturity(level) {
  return level === 'H5' ? 'M6' : 'M5';
}

function parseJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function tail(text, lines = 10) {
  return text ? text.split('\n').slice(-lines).join('\n').trim() : '';
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

main();
