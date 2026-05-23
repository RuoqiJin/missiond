#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

import {
  RUNTIME_DOMAIN_FILES,
  RUNTIME_DOMAIN_SOURCE_HASHES,
} from './generated/v3_contracts.mjs';
import {
  runSemanticRules,
  sourceDomainBundleHash,
} from './lib/v3_semantic_rules.mjs';

const COMPILED_DIR = '.missiond/v3/runtime/compiled';
const SCHEMA_VERSION = 'missiond.compiled-runtime-domain.v1';
const WORKSTATION_DOMAIN_FILE_ANCHOR = 'compiled-runtime-workstation.json';

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repoRoot = path.resolve(opts.repo);
  const diagnostics = [];
  const domains = [];

  for (const [domain, file] of Object.entries(RUNTIME_DOMAIN_FILES ?? {}).sort()) {
    const rel = path.join(COMPILED_DIR, file);
    const abs = path.join(repoRoot, rel);
    let compiled;
    try {
      compiled = JSON.parse(fs.readFileSync(abs, 'utf8'));
    } catch (err) {
      diagnostics.push(diag(rel, 'RUNTIME_DOMAIN_FILE_MISSING', `cannot read runtime domain projection: ${err.message}`));
      continue;
    }

    if (compiled?.schema_version !== SCHEMA_VERSION) {
      diagnostics.push(diag(rel, 'RUNTIME_DOMAIN_SCHEMA_MISMATCH', `expected ${SCHEMA_VERSION}, got ${JSON.stringify(compiled?.schema_version)}`));
    }
    if (compiled?.payload?.domain !== domain) {
      diagnostics.push(diag(rel, 'RUNTIME_DOMAIN_ID_MISMATCH', `expected payload.domain ${domain}, got ${JSON.stringify(compiled?.payload?.domain)}`));
    }
    if (!compiled?.payload?.config || typeof compiled.payload.config !== 'object' || Array.isArray(compiled.payload.config)) {
      diagnostics.push(diag(rel, 'RUNTIME_DOMAIN_CONFIG_MISSING', 'payload.config must be an object'));
    }
    const sourceDomains = compiled?.payload?.source_domains;
    const actualSourceHash = Array.isArray(sourceDomains) ? sourceDomainBundleHash(sourceDomains) : null;
    const expectedSourceHash = RUNTIME_DOMAIN_SOURCE_HASHES?.[domain];
    if (!actualSourceHash) {
      diagnostics.push(diag(rel, 'RUNTIME_DOMAIN_SOURCE_DOMAINS_MISSING', 'payload.source_domains must be non-empty'));
    } else if (expectedSourceHash && actualSourceHash !== expectedSourceHash) {
      diagnostics.push(diag(rel, 'RUNTIME_DOMAIN_SOURCE_HASH_STALE', `expected source domain hash ${expectedSourceHash}, got ${actualSourceHash}; run node scripts/project-v3-contracts.mjs --write and node scripts/compile-v3-runtime.mjs --json`));
    }
    diagnostics.push(...runSemanticRules({
      rules: ['source-domain-hash-consistency'],
      file: rel,
      compiledTargets: [{ id: `runtime-domain:${domain}`, compiled }],
    }));
    domains.push({
      domain,
      file: rel,
      source_hash: compiled?.source_hash ?? null,
      source_domain_hash: actualSourceHash,
    });
  }

  const ok = diagnostics.length === 0;
  const output = {
    ok,
    schema: 'missiond.runtime-domain-projection-check.v1',
    domains,
    diagnostics,
  };
  if (opts.json) console.log(JSON.stringify(output, null, 2));
  else if (ok) console.log(`V3 runtime domain projections OK (${domains.length} domains)`);
  else {
    for (const d of diagnostics) console.error(`${d.file}:${d.line}:${d.column}: ${d.code}: ${d.message}`);
    console.error(`V3 runtime domain projections FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, repo: process.cwd() };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--repo') opts.repo = argv[++i] ?? fail('--repo requires a value');
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '-h' || arg === '--help') {
      console.log('Usage: node scripts/check-v3-runtime-domain-projections.mjs [--json] [--repo <path>]');
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

function diag(file, code, message) {
  return {
    severity: 'error',
    file,
    line: 1,
    column: 1,
    code,
    message,
  };
}

main();
