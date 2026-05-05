#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-missiond-blue-green-deploy.mjs [--json] [--dry-fixture]

Checks the MissionD self-update blue-green contract:
  - V3 declares missiond-blue-green-self-update.
  - deploy-daemon.sh installs paired daemon/MCP release dirs, switches active,
    rolls back on smoke failure, and supports safe cleanup.
`;

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  deploy: 'scripts/deploy-daemon.sh',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
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

  const root = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = check(root);
  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('missiond blue-green deploy check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`missiond blue-green deploy check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function check(root) {
  const diagnostics = [];
  const blueprint = read(root, FILES.blueprint, diagnostics, true);
  const deploy = read(root, FILES.deploy, diagnostics, false);
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, FILES.blueprint, blueprint, [
    '(surface missiond-blue-green-self-update',
    ':implements [blue-green-self-update release-manifest release-cleanup rollback]',
    'scripts/check-missiond-blue-green-deploy.mjs',
    'Release candidates are immutable directories under ~/.xjp-mission/releases/<release-id>',
    'daemon and MCP entrypoints both resolve through active',
  ]);

  requireAll(diagnostics, FILES.deploy, deploy, [
    'MISSIOND_INSTALL_ROOT',
    'MISSIOND_RELEASES_DIR',
    'MISSIOND_ACTIVE_LINK',
    'MISSIOND_RELEASE_KEEP',
    'MISSIOND_BACKUP_RETENTION_DAYS',
    'CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"',
    'release-manifest.json',
    '"schema":"missiond.release-manifest.v1"',
    'daemon_sha256',
    'mcp_sha256',
    'atomic_symlink_update',
    'switch_active_release',
    'rollback_to_previous',
    'cleanup_old_releases',
    'release_complete',
    'removed incomplete release',
    'create_legacy_release_if_needed',
    'codesign_or_verify',
    'force-sign failed but verified linker signature',
    'pre-switch smoke: candidate MCP initialize',
    '$ACTIVE_LINK/bin/missiond',
    '$ACTIVE_LINK/bin/mission-mcp',
    'active_release=$RELEASE_ID',
    '--cleanup-only',
    '--apply-cleanup',
  ]);

  return diagnostics;
}

function read(root, rel, diagnostics, blueprint) {
  try {
    return blueprint
      ? readBlueprintWithEvidenceSidecars(root, rel)
      : fs.readFileSync(path.join(root, rel), 'utf8');
  } catch (err) {
    diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    return '';
  }
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push({ file, message: `missing required text: ${needle}` });
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-blue-green-fixture-'));
  write(root, FILES.blueprint, `
(missiond-blueprint
  (implementation-map
    (surface missiond-blue-green-self-update
      :status "code-aligned"
      :implements [blue-green-self-update release-manifest release-cleanup rollback]
      :code ["scripts/deploy-daemon.sh" "scripts/check-missiond-blue-green-deploy.mjs"]
      :note "Release candidates are immutable directories under ~/.xjp-mission/releases/<release-id>; daemon and MCP entrypoints both resolve through active.")))`);
  write(root, FILES.deploy, `
MISSIOND_INSTALL_ROOT MISSIOND_RELEASES_DIR MISSIOND_ACTIVE_LINK MISSIOND_RELEASE_KEEP MISSIOND_BACKUP_RETENTION_DAYS
release-manifest.json "schema":"missiond.release-manifest.v1" daemon_sha256 mcp_sha256
atomic_symlink_update switch_active_release rollback_to_previous cleanup_old_releases create_legacy_release_if_needed
codesign_or_verify force-sign failed but verified linker signature
pre-switch smoke: candidate MCP initialize
$ACTIVE_LINK/bin/missiond
$ACTIVE_LINK/bin/mission-mcp
active_release=$RELEASE_ID
--cleanup-only --apply-cleanup
`);
  return root;
}

function write(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content.trimStart());
}

main();
