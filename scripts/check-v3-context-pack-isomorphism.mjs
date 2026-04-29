#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const usage = `Usage:
  node scripts/check-v3-context-pack-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 context-pack Lisp/code isomorphism contract:
  - blueprint declares multi-agent append-only context-pack architecture.
  - implementation-map exposes context-pack as a code-aligned surface.
  - scripts/check-context-pack.mjs owns structural validation for context-pack v1.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  checker: 'scripts/check-context-pack.mjs',
  appender: 'scripts/context-pack-append.mjs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  if (diagnostics.length === 0) {
    for (const script of [DEFAULT_FILES.checker, DEFAULT_FILES.appender]) {
      const proc = spawnSync(process.execPath, [path.join(repoRoot, script), '--dry-fixture'], {
        cwd: repoRoot,
        encoding: 'utf8',
        timeout: 30_000,
      });
      if (proc.status !== 0 || proc.error) {
        diagnostics.push({
          file: script,
          message: `dry fixture failed: ${proc.error?.message ?? proc.stderr ?? proc.stdout}`,
        });
      }
    }
  }

  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 context-pack Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(
      `v3 context-pack Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }
  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'context-pack',
    'missiond.context-pack.v1',
    'multi-agent append-only',
    'shard-proposal',
    'integration-plan',
    'accepted-shards',
    'dispatch-groups',
    '(surface context-pack',
    ':status "code-aligned"',
    'scripts/check-context-pack.mjs',
    'scripts/context-pack-append.mjs',
    'node scripts/check-v3-context-pack-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.checker, sources.checker, [
    'export const SCHEMA = \'missiond.context-pack.v1\'',
    'export const CONTEXT_PACK_HEAD = \'context-pack\'',
    'export const ENTRY_HEADS',
    '\'shard-proposal\'',
    '\'integration-plan\'',
    'validateContextPackSource',
    'validateContextPackFiles',
    'append-only',
    'accepted shards',
    'write-scope',
    'runFixtures',
  ]);

  requireAll(diagnostics, files.appender, sources.appender, [
    'appendContextPackEntry',
    'withLock',
    'nextSequence',
    'validateContextPackSource',
    'spliceBeforeFinalParen',
    'context-pack-append OK',
    '--pack <context-pack.lisp>',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-context-pack-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (artifact-contracts
    (artifact context-pack
      :schema "missiond.context-pack.v1"
      :writer context-pack-append
      :required [:id :agent :seq]))
  (multi-agent-context-pack
    :write-model "multi-agent append-only"
    :entries [claim observation anchor shard-proposal conflict integration-plan]
    :merge "accepted-shards become dispatch-groups")
  (implementation-map
    (surface context-pack
      :status "code-aligned"
      :code ["scripts/check-context-pack.mjs"
             "scripts/context-pack-append.mjs"]
      :note "fixture"))
  (compression-contract
    :checks ["node scripts/check-v3-context-pack-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.checker, fs.readFileSync(DEFAULT_FILES.checker, 'utf8'));
  writeFixture(root, DEFAULT_FILES.appender, fs.readFileSync(DEFAULT_FILES.appender, 'utf8'));
  writeFixture(root, 'scripts/lib/missiond_lisp.mjs', fs.readFileSync('scripts/lib/missiond_lisp.mjs', 'utf8'));
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
