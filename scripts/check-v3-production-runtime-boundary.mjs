#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const FILES = {
  rustRuntime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  jsRuntime: 'scripts/lib/v3_workstation_runtime.mjs',
  contractProjector: 'scripts/project-v3-contracts.mjs',
};

function main() {
  const json = process.argv.includes('--json');
  const repo = process.cwd();
  const diagnostics = [];
  const sources = Object.fromEntries(
    Object.entries(FILES).map(([key, rel]) => [key, read(repo, rel, diagnostics)]),
  );
  if (diagnostics.length === 0) {
    checkDefaultConstants(FILES.rustRuntime, sources.rustRuntime, diagnostics, /^pub\(crate\) const DEFAULT_/);
    checkDefaultConstants(FILES.jsRuntime, sources.jsRuntime, diagnostics, /^export const DEFAULT_/);
    requireAll(FILES.rustRuntime, sources.rustRuntime, diagnostics, [
      'required_compiled_runtime_config',
      'compiled runtime config is required',
      'cfg!(debug_assertions) || cfg!(test)',
      'MISSIOND_V3_ALLOW_SOURCE_FALLBACK',
      'return false;',
      'load_compiled_runtime_config',
      'v3_contracts::SOURCE_HASH',
    ]);
    requireAll(FILES.jsRuntime, sources.jsRuntime, diagnostics, [
      'COMPILED_RUNTIME_CONFIG_REL',
      'compiled runtime config is required',
      'MISSIOND_V3_ALLOW_SOURCE_FALLBACK',
      'V3_CONTRACT_SOURCE_HASH',
    ]);
    requireAll(FILES.contractProjector, sources.contractProjector, diagnostics, [
      'RuntimePolicyDescriptor',
      'RUNTIME_POLICIES',
      'payload_key',
      'checkerRegistry',
    ]);
  }

  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('v3 production runtime boundary check OK');
  else for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
  process.exit(result.ok ? 0 : 1);
}

function read(repo, rel, diagnostics) {
  try {
    return fs.readFileSync(path.join(repo, rel), 'utf8');
  } catch (err) {
    diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    return '';
  }
}

function checkDefaultConstants(file, source, diagnostics, prefixRe) {
  const offenders = source
    .split('\n')
    .map((line, idx) => ({ line, idx: idx + 1 }))
    .filter(({ line }) => prefixRe.test(line.trim()) && /\/Users\//.test(line));
  for (const offender of offenders) {
    diagnostics.push({
      file,
      line: offender.idx,
      column: 1,
      code: 'HOST_SPECIFIC_PRODUCTION_DEFAULT',
      message: `production DEFAULT_* constant must not embed a host path: ${offender.line.trim()}`,
    });
  }
}

function requireAll(file, source, diagnostics, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({
        file,
        line: 1,
        column: 1,
        code: 'PRODUCTION_RUNTIME_BOUNDARY_MISSING',
        message: `missing production runtime boundary anchor: ${needle}`,
      });
    }
  }
}

main();
