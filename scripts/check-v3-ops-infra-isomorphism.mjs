#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-ops-infra-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 ops-infra Lisp/code isomorphism contract:
  - deploy-daemon is the canonical one-command blue-green redeploy path.
  - deploy-daemon keeps build, candidate release, manifest, active symlink,
    kickstart, socket wait, bounded IPC smoke, rollback, and cleanup together.
  - cargo-fmt-touched formats only Rust files present in the current diff.
  - cargo-fmt-touched skips only explicit missiond-rustfmt-exempt legacy
    facades while V3 physical split is in progress.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  deployDaemon: 'scripts/deploy-daemon.sh',
  cargoFmtTouched: 'scripts/cargo-fmt-touched.sh',
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

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 ops-infra Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(`v3 ops-infra Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'ops-infra',
    '(surface ops-infra',
    ':status "code-aligned"',
    'scripts/deploy-daemon.sh',
    'scripts/cargo-fmt-touched.sh',
    'scripts/check-v3-ops-infra-isomorphism.mjs',
    'Daemon redeploy MUST stay one command',
    'build -> candidate release -> manifest -> active symlink -> launchctl kickstart -> socket wait -> IPC smoke',
    'Active daemon and MCP entrypoints MUST resolve through ~/.xjp-mission/active',
    'Blue-green rollback MUST switch active back to the previous release',
    'Release cleanup MUST keep active, previous, and newest retained releases',
    'IPC smoke MUST retry after socket readiness and then rollback on failure',
    'Deploy smoke timeout MUST be configurable through MISSIOND_DEPLOY_SMOKE_TIMEOUT',
    'Rust formatting MUST be scoped to Rust files touched in the current diff',
    'missiond-rustfmt-exempt legacy-large-file facades',
    'rustfmt MUST run with skip_children=true',
    'node scripts/check-v3-ops-infra-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.deployDaemon, sources.deployDaemon, [
    'scripts/deploy-daemon.sh                  # build + blue-green deploy + smoke',
    '--build-only',
    '--no-smoke',
    '--debug',
    '--cleanup-only',
    '--apply-cleanup',
    'MISSIOND_INSTALL_ROOT',
    'MISSIOND_RELEASES_DIR',
    'MISSIOND_ACTIVE_LINK',
    'MISSIOND_RELEASE_KEEP',
    'MISSIOND_BACKUP_RETENTION_DAYS',
    'MISSIOND_BIN_PATH',
    'MISSIOND_MCP_BIN_PATH',
    'MISSIOND_SOCKET_PATH',
    'MISSIOND_LAUNCHCTL_LABEL',
    'MISSIOND_DEPLOY_TIMEOUT',
    'MISSIOND_DEPLOY_SMOKE_TIMEOUT',
    'set -euo pipefail',
    'cargo build ${BUILD_ARG} -p missiond-daemon -p missiond-mcp',
    'release-manifest.json',
    'atomic_symlink_update',
    'switch_active_release',
    'rollback_to_previous',
    'cleanup_old_releases',
    'create_legacy_release_if_needed',
    'codesign --force --sign - "$CANDIDATE_DIR/bin/missiond"',
    'codesign --force --sign - "$CANDIDATE_DIR/bin/mission-mcp"',
    'launchctl kickstart -k "gui/$(id -u)/$LABEL"',
    'lsof "$SOCK_PATH"',
    'run_mcp_initialize_smoke()',
    'command -v timeout',
    'command -v gtimeout',
    "perl -e 'alarm shift @ARGV; exec @ARGV'",
    'smoke_start="$(date +%s)"',
    'SMOKE_TIMEOUT="${MISSIOND_DEPLOY_SMOKE_TIMEOUT:-30}"',
    'IPC not ready yet; retrying',
    'active_release=$RELEASE_ID',
    'rollback attempted',
  ]);

  requireAll(diagnostics, files.cargoFmtTouched, sources.cargoFmtTouched, [
    'scripts/cargo-fmt-touched.sh --check',
    'scripts/cargo-fmt-touched.sh --staged',
    'scripts/cargo-fmt-touched.sh --branch main',
    'MODE="all"',
    'CHECK_ONLY=0',
    'git diff --name-only --diff-filter=ACMR',
    'git diff --cached --name-only --diff-filter=ACMR',
    'git diff --name-only --diff-filter=ACMR "${BRANCH}...HEAD"',
    "awk '/\\.rs$/ { print }'",
    'no Rust files in diff',
    'missiond-rustfmt-exempt',
    'skipped rustfmt-exempt legacy file(s)',
    'command -v rustfmt',
    '--config skip_children=true --check',
    'xargs rustfmt --edition "$EDITION" --config skip_children=true',
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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-ops-infra-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (ops-infra
    :invariants
      ["Daemon redeploy MUST stay one command: build -> candidate release -> manifest -> active symlink -> launchctl kickstart -> socket wait -> IPC smoke."
       "Active daemon and MCP entrypoints MUST resolve through ~/.xjp-mission/active."
       "Blue-green rollback MUST switch active back to the previous release."
       "Release cleanup MUST keep active, previous, and newest retained releases."
       "IPC smoke MUST retry after socket readiness and then rollback on failure."
       "Deploy smoke timeout MUST be configurable through MISSIOND_DEPLOY_SMOKE_TIMEOUT."
       "Rust formatting MUST be scoped to Rust files touched in the current diff."
       "missiond-rustfmt-exempt legacy-large-file facades are skipped only during physical V3 split."
       "rustfmt MUST run with skip_children=true."])
  (implementation-map
    (surface ops-infra
      :status "code-aligned"
      :code ["scripts/deploy-daemon.sh"
             "scripts/cargo-fmt-touched.sh"
             "scripts/check-v3-ops-infra-isomorphism.mjs"]
      :note "fixture"))
  (compression-contract
    :checks ["node scripts/check-v3-ops-infra-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.deployDaemon, `
scripts/deploy-daemon.sh                  # build + blue-green deploy + smoke
--build-only --no-smoke --debug --cleanup-only --apply-cleanup
MISSIOND_INSTALL_ROOT MISSIOND_RELEASES_DIR MISSIOND_ACTIVE_LINK MISSIOND_RELEASE_KEEP MISSIOND_BACKUP_RETENTION_DAYS
MISSIOND_BIN_PATH MISSIOND_MCP_BIN_PATH MISSIOND_SOCKET_PATH MISSIOND_LAUNCHCTL_LABEL MISSIOND_DEPLOY_TIMEOUT MISSIOND_DEPLOY_SMOKE_TIMEOUT
set -euo pipefail
cargo build \${BUILD_ARG} -p missiond-daemon -p missiond-mcp
release-manifest.json
atomic_symlink_update
switch_active_release
rollback_to_previous
cleanup_old_releases
create_legacy_release_if_needed
codesign --force --sign - "$CANDIDATE_DIR/bin/missiond"
codesign --force --sign - "$CANDIDATE_DIR/bin/mission-mcp"
launchctl kickstart -k "gui/$(id -u)/$LABEL"
lsof "$SOCK_PATH"
run_mcp_initialize_smoke()
command -v timeout
command -v gtimeout
perl -e 'alarm shift @ARGV; exec @ARGV'
smoke_start="$(date +%s)"
SMOKE_TIMEOUT="\${MISSIOND_DEPLOY_SMOKE_TIMEOUT:-30}"
IPC not ready yet; retrying
active_release=$RELEASE_ID
rollback attempted
`);

  writeFixture(root, DEFAULT_FILES.cargoFmtTouched, `
scripts/cargo-fmt-touched.sh --check
scripts/cargo-fmt-touched.sh --staged
scripts/cargo-fmt-touched.sh --branch main
MODE="all"
CHECK_ONLY=0
git diff --name-only --diff-filter=ACMR
git diff --cached --name-only --diff-filter=ACMR
git diff --name-only --diff-filter=ACMR "\${BRANCH}...HEAD"
awk '/\\.rs$/ { print }'
no Rust files in diff
missiond-rustfmt-exempt
skipped rustfmt-exempt legacy file(s)
command -v rustfmt
--config skip_children=true --check
xargs rustfmt --edition "$EDITION" --config skip_children=true
`);

  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content.trimStart());
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
