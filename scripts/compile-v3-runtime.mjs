#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { runLispc } from './lib/ocaml_lispc.mjs';

const OUT_DIR = '.missiond/v3/runtime/compiled';
const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const WORKFLOW_DIR = '.missiond/workflows';
const GENOME_DIR = '.missiond/v3/genome';

const targets = [
  {
    id: 'v3',
    argv: ['emit-v3', '--blueprint', BLUEPRINT],
    file: 'compiled-v3-blueprint.json',
  },
  {
    id: 'runtime-config',
    argv: ['emit-runtime-config', '--blueprint', BLUEPRINT],
    file: 'compiled-runtime-config.json',
  },
  {
    id: 'semantic-ir',
    argv: ['emit-semantic-ir', '--blueprint', BLUEPRINT],
    file: 'compiled-semantic-ir.json',
  },
  {
    id: 'universe',
    argv: ['emit-universe', '--blueprint', BLUEPRINT],
    file: 'compiled-project-universe.json',
  },
  {
    id: 'workflows',
    argv: ['emit-workflows', '--workflow-dir', WORKFLOW_DIR],
    file: 'compiled-workflows.json',
  },
  {
    id: 'genomes',
    argv: ['emit-genomes', '--genome-dir', GENOME_DIR],
    file: 'compiled-genomes.json',
  },
];

function main() {
  const opts = parseArgs(process.argv.slice(2));
  fs.mkdirSync(opts.outDir, { recursive: true });
  const results = [];
  for (const target of targets) {
    const result = runLispc(target.argv);
    const compiled = result?.compiled;
    if (!result?.ok || !compiled) {
      results.push({
        id: target.id,
        ok: false,
        diagnostics: result?.diagnostics ?? [],
        stderr: result?.stderr ?? '',
      });
      continue;
    }
    const outPath = path.join(opts.outDir, target.file);
    fs.writeFileSync(outPath, `${JSON.stringify(compiled, null, 2)}\n`);
    results.push({ id: target.id, ok: true, path: outPath, source_hash: compiled.source_hash });
  }
  const semantic = results.find((row) => row.id === 'semantic-ir' && row.ok);
  if (semantic) {
    const semanticPath = path.join(opts.outDir, 'compiled-semantic-ir.json');
    const semanticJson = JSON.parse(fs.readFileSync(semanticPath, 'utf8'));
    const facts = semanticJson?.payload?.facts ?? [];
    const slices = {
      schema_version: 'missiond.compiled-agent-slices.v1',
      source_hash: semanticJson.source_hash,
      generated_at: null,
      diagnostics: semanticJson.diagnostics ?? [],
      payload: {
        slice_policy: 'agents receive compact fact slices plus accepted shard metadata before full Lisp',
        facts,
      },
    };
    const slicePath = path.join(opts.outDir, 'compiled-agent-slices.json');
    fs.writeFileSync(slicePath, `${JSON.stringify(slices, null, 2)}\n`);
    results.push({ id: 'agent-slices', ok: true, path: slicePath, source_hash: semanticJson.source_hash });
  }
  const workflows = results.find((row) => row.id === 'workflows' && row.ok);
  if (workflows) {
    const workflowsPath = path.join(opts.outDir, 'compiled-workflows.json');
    const workflowsJson = JSON.parse(fs.readFileSync(workflowsPath, 'utf8'));
    const contracts = {
      schema_version: 'missiond.compiled-workflow-contracts.v1',
      source_hash: workflowsJson.source_hash,
      generated_at: null,
      diagnostics: workflowsJson.diagnostics ?? [],
      payload: workflowsJson.payload,
    };
    const contractsPath = path.join(opts.outDir, 'compiled-workflow-contracts.json');
    fs.writeFileSync(contractsPath, `${JSON.stringify(contracts, null, 2)}\n`);
    results.push({ id: 'workflow-contracts', ok: true, path: contractsPath, source_hash: workflowsJson.source_hash });
  }
  const ok = results.every((row) => row.ok);
  const payload = { ok, out_dir: opts.outDir, results };
  if (opts.json) console.log(JSON.stringify(payload, null, 2));
  else if (ok) {
    for (const row of results) console.log(`${row.id}: ${row.path}`);
  } else {
    console.error(JSON.stringify(payload, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, outDir: OUT_DIR };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--out-dir') opts.outDir = argv[++i] ?? fail('--out-dir requires a value');
    else if (arg.startsWith('--out-dir=')) opts.outDir = arg.slice('--out-dir='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/compile-v3-runtime.mjs [--json] [--out-dir <dir>]');
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
