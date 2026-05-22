#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
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
    id: 'contract-abi',
    argv: ['emit-contract-abi', '--blueprint', BLUEPRINT],
    file: 'compiled-contract-abi.json',
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
  const outDir = opts.check && opts.outDir === OUT_DIR
    ? fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-runtime-check-'))
    : opts.outDir;
  fs.mkdirSync(outDir, { recursive: true });
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
    const outPath = path.join(outDir, target.file);
    fs.writeFileSync(outPath, `${JSON.stringify(compiled, null, 2)}\n`);
    results.push({ id: target.id, ok: true, path: outPath, source_hash: compiled.source_hash });
  }
  const ssotHashRows = results.filter((row) => (
    row.ok && ['v3', 'runtime-config', 'semantic-ir', 'contract-abi'].includes(row.id)
  ));
  if (ssotHashRows.length === 4) {
    const hashes = new Set(ssotHashRows.map((row) => row.source_hash));
    if (hashes.size !== 1) {
      results.push({
        id: 'source-hash-consistency',
        ok: false,
        diagnostics: [{
          message: `compiled V3 SSOT source_hash mismatch: ${ssotHashRows.map((row) => `${row.id}=${row.source_hash}`).join(', ')}`,
        }],
      });
    }
    const sourceUnitsRows = ssotHashRows.map((row) => {
      const compiledPath = path.join(outDir, targets.find((target) => target.id === row.id).file);
      const compiled = JSON.parse(fs.readFileSync(compiledPath, 'utf8'));
      return {
        id: row.id,
        count: Array.isArray(compiled?.payload?.source_units) ? compiled.payload.source_units.length : 0,
        source_units: normalizeSourceUnits(compiled?.payload?.source_units),
      };
    });
    for (const row of sourceUnitsRows) {
      if (row.count === 0) {
        results.push({
          id: `${row.id}-source-units-present`,
          ok: false,
          diagnostics: [{
            message: `${row.id} compiled payload must include non-empty source_units`,
          }],
        });
      }
    }
    const reference = sourceUnitsRows[0];
    const mismatches = sourceUnitsRows
      .filter((row) => row.source_units !== reference.source_units)
      .map((row) => `${row.id} source_units differ from ${reference.id}`);
    if (mismatches.length > 0) {
      results.push({
        id: 'source-units-consistency',
        ok: false,
        diagnostics: mismatches.map((message) => ({ message })),
      });
    }
  }
  const semantic = results.find((row) => row.id === 'semantic-ir' && row.ok);
  if (semantic) {
    const semanticPath = path.join(outDir, 'compiled-semantic-ir.json');
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
    const slicePath = path.join(outDir, 'compiled-agent-slices.json');
    fs.writeFileSync(slicePath, `${JSON.stringify(slices, null, 2)}\n`);
    results.push({ id: 'agent-slices', ok: true, path: slicePath, source_hash: semanticJson.source_hash });
  }
  const workflows = results.find((row) => row.id === 'workflows' && row.ok);
  if (workflows) {
    const workflowsPath = path.join(outDir, 'compiled-workflows.json');
    const workflowsJson = JSON.parse(fs.readFileSync(workflowsPath, 'utf8'));
    const contracts = {
      schema_version: 'missiond.compiled-workflow-contracts.v1',
      source_hash: workflowsJson.source_hash,
      generated_at: null,
      diagnostics: workflowsJson.diagnostics ?? [],
      payload: workflowsJson.payload,
    };
    const contractsPath = path.join(outDir, 'compiled-workflow-contracts.json');
    fs.writeFileSync(contractsPath, `${JSON.stringify(contracts, null, 2)}\n`);
    results.push({ id: 'workflow-contracts', ok: true, path: contractsPath, source_hash: workflowsJson.source_hash });
  }
  const ok = results.every((row) => row.ok);
  const payload = { ok, mode: opts.check ? 'check' : 'write', out_dir: outDir, results };
  if (opts.json) console.log(JSON.stringify(payload, null, 2));
  else if (ok) {
    for (const row of results) console.log(`${row.id}: ${row.path}`);
  } else {
    console.error(JSON.stringify(payload, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, check: false, write: false, outDir: OUT_DIR };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--check') opts.check = true;
    else if (arg === '--write') opts.write = true;
    else if (arg === '--out-dir') opts.outDir = argv[++i] ?? fail('--out-dir requires a value');
    else if (arg.startsWith('--out-dir=')) opts.outDir = arg.slice('--out-dir='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/compile-v3-runtime.mjs [--json] [--check|--write] [--out-dir <dir>]');
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (opts.check && opts.write) fail('--check and --write are mutually exclusive');
  return opts;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

function normalizeSourceUnits(sourceUnits) {
  if (!Array.isArray(sourceUnits)) return '[]';
  return JSON.stringify(sourceUnits.map((unit) => ({
    file: unit?.file ?? null,
    kind: unit?.kind ?? null,
    included_by: unit?.included_by ?? null,
    include_line: unit?.include_line ?? null,
    source_hash: unit?.source_hash ?? null,
  })));
}

main();
