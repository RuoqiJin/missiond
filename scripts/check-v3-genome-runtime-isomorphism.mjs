#!/usr/bin/env node

import fs from 'node:fs';
import { runLispc } from './lib/ocaml_lispc.mjs';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const GENOME_DIR = '.missiond/v3/genome';

const REQUIRED_FILES = [
  '.missiond/v3/genome/autopilot.lisp',
  'tools/missiond_lispc/bin/genome_schema.ml',
  'crates/missiond-kernel/src/lib.rs',
  'crates/missiond-genome/src/lib.rs',
  'crates/missiond-organism-runtime/src/lib.rs',
  'crates/missiond-organism-runtime/src/autopilot.rs',
  'crates/missiond-daemon/src/organism/autopilot_organ.rs',
];

const BLUEPRINT_TOKENS = [
  '(function genome-runtime',
  ':surface genome-runtime',
  '(surface genome-runtime',
  'compiled-genomes',
  '.missiond/v3/runtime/compiled/compiled-genomes.json',
  'missiond-lispc.check-genome-dir',
  'missiond-lispc.emit-genomes',
];

const RUST_KERNEL_TOKENS = [
  'pub struct EventEnvelope',
  'pub enum Effect',
  'pub struct CommandEnvelope',
  'pub trait Atom',
  'pub struct AtomRegistry',
  'pub struct Molecule',
  'pub struct RuleGraph',
  'pub trait Cell',
  'pub struct TissueProfile',
  'pub struct Genome',
  'pub enum ActivationMode',
  'DEFAULT_MAX_CAUSATION_DEPTH: u8 = 10',
  'DEFAULT_MAX_EVENTS_PER_CORRELATION: u32 = 128',
  'DEFAULT_MAX_CELL_RUNTIME_MS: u64 = 300_000',
  'DEFAULT_IDEMPOTENCY_CACHE_SIZE: usize = 4_096',
];

const RUNTIME_TOKENS = [
  'pub struct CellRuntime',
  'pub trait EffectInterpreter',
  'ActivationMode::Shadow',
  'ActivationMode::Active',
  'ActivationMode::Rollback',
  'RuntimeGuard',
  'IdempotencyWindow',
];

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = [];

  for (const file of REQUIRED_FILES) {
    if (!fs.existsSync(file)) {
      diagnostics.push(diag(file, 'FILE_MISSING', 'required genome runtime file is missing'));
    }
  }

  const blueprint = read(BLUEPRINT);
  for (const token of BLUEPRINT_TOKENS) {
    if (!blueprint.includes(token)) {
      diagnostics.push(diag(BLUEPRINT, 'BLUEPRINT_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
    }
  }

  const kernel = read('crates/missiond-kernel/src/lib.rs');
  for (const token of RUST_KERNEL_TOKENS) {
    if (!kernel.includes(token)) {
      diagnostics.push(diag('crates/missiond-kernel/src/lib.rs', 'KERNEL_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
    }
  }

  const runtime = read('crates/missiond-organism-runtime/src/lib.rs');
  for (const token of RUNTIME_TOKENS) {
    if (!runtime.includes(token)) {
      diagnostics.push(diag('crates/missiond-organism-runtime/src/lib.rs', 'RUNTIME_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
    }
  }

  const check = runLispc(['check-genome-dir', '--genome-dir', GENOME_DIR]);
  if (!check.ok) {
    diagnostics.push(diag('tools/missiond_lispc/bin/genome_schema.ml', 'GENOME_CHECK_FAILED', 'missiond-lispc check-genome-dir failed'));
    for (const d of check.diagnostics ?? []) diagnostics.push({ ...d, code: d.code ?? 'GENOME_DIAGNOSTIC' });
  }

  const emit = runLispc(['emit-genomes', '--genome-dir', GENOME_DIR]);
  if (!emit.ok || !emit.compiled) {
    diagnostics.push(diag('tools/missiond_lispc/bin/genome_schema.ml', 'GENOME_EMIT_FAILED', 'missiond-lispc emit-genomes must return compiled JSON'));
  } else {
    const genomes = emit.compiled?.payload?.genomes ?? [];
    if (!Array.isArray(genomes) || genomes.length === 0) {
      diagnostics.push(diag('tools/missiond_lispc/bin/genome_schema.ml', 'GENOME_EMIT_EMPTY', 'compiled-genomes payload must include genomes[]'));
    }
  }

  const result = {
    ok: diagnostics.length === 0,
    diagnostics,
    checked_files: REQUIRED_FILES,
  };
  if (opts.json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('V3 genome runtime isomorphism OK');
  else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.code}: ${d.message}`);
    console.error('V3 genome runtime isomorphism FAILED');
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false };
  for (const arg of argv) {
    if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/check-v3-genome-runtime-isomorphism.mjs [--json]');
      process.exit(0);
    } else {
      console.error(`unknown argument: ${arg}`);
      process.exit(2);
    }
  }
  return opts;
}

function read(file) {
  try {
    if (file === BLUEPRINT) return readBlueprintWithEvidenceSidecars(process.cwd(), file);
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

function diag(file, code, message) {
  return { file, line: 1, column: 1, code, message, path: '' };
}

main();
