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
  - rustfmt-missiond proves MissionD-owned Rust crates are formatter-converged.
  - cargo-fmt-touched remains the scoped fallback for non-M6/external projects.
  - MissionD's runtime database is PostgreSQL-only; SQLite may appear only in
    the external Codex provider-local state_5.sqlite index adapter.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  deployDaemon: 'scripts/deploy-daemon.sh',
  rustfmtMissiond: 'scripts/rustfmt-missiond.sh',
  cargoFmtTouched: 'scripts/cargo-fmt-touched.sh',
  daemonMain: 'crates/missiond-daemon/src/main.rs',
  astSyncWorker: 'crates/missiond-daemon/src/workers/local/ast_sync_worker.rs',
  workspaceCargo: 'Cargo.toml',
  daemonCargo: 'crates/missiond-daemon/Cargo.toml',
  corePgMod: 'crates/missiond-core/src/db/pg/mod.rs',
  coreDbTraits: 'crates/missiond-core/src/db/traits.rs',
  codexIngestionWorker: 'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
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
    'scripts/rustfmt-missiond.sh',
    'scripts/cargo-fmt-touched.sh',
    'scripts/check-v3-ops-infra-isomorphism.mjs',
    'Daemon redeploy MUST stay one command',
    'build -> candidate release -> manifest -> active symlink -> launchctl kickstart -> socket wait -> IPC smoke',
    'Active daemon and MCP entrypoints MUST resolve through ~/.xjp-mission/active',
    'Blue-green rollback MUST switch active back to the previous release',
    'Blue-green rollback MUST restore launchd WorkingDirectory, MISSIOND_PROJECT_ROOT, MISSIOND_RUNTIME_DIR, and MISSIOND_COMPILED_RUNTIME_DIR captured before the switch',
    'Release cleanup MUST keep active, previous, and newest retained releases',
    'IPC smoke MUST retry after socket readiness and then rollback on failure',
    'Deploy smoke timeout MUST be configurable through MISSIOND_DEPLOY_SMOKE_TIMEOUT',
    'Deploy scripts MUST emit timing for cargo-build',
    'Dev-only fast deploy may select debug profile and sccache',
    'AST repository-wide startup full sync MUST be opt-in through MISSIOND_AST_FULL_SYNC_ON_STARTUP',
    'M6 MissionD formatting MUST be converged',
    'Rust formatter edition MUST be derived from workspace Cargo.toml',
    'scripts/rustfmt-missiond.sh --check',
    'No MissionD Rust source may carry formatter exemption markers',
    'Rust formatting for external or non-M6 projects MAY remain scoped',
    'rustfmt MUST run with skip_children=true',
    'MissionD primary runtime database MUST be PostgreSQL-only',
    'Applied Postgres migration files are immutable, including comments',
    'old MissionD SQLite backend, SQLite-to-Postgres migration module, and sqlite feature cfg MUST be absent',
    'SQLite references are allowed only for the external Codex provider-local state_5.sqlite index adapter',
    'node scripts/check-v3-ops-infra-isomorphism.mjs',
    'node scripts/check-pg-migrations-discipline.mjs',
  ]);

  requireAll(diagnostics, files.deployDaemon, sources.deployDaemon, [
    'scripts/deploy-daemon.sh                  # build + blue-green deploy + smoke',
    '--build-only',
    '--no-smoke',
    '--debug',
    '--fast',
    '--cleanup-only',
    '--apply-cleanup',
    'MISSIOND_USE_SCCACHE',
    'MISSIOND_INSTALL_ROOT',
    'MISSIOND_RELEASES_DIR',
    'MISSIOND_ACTIVE_LINK',
    'MISSIOND_RELEASE_KEEP',
    'PREVIOUS_LAUNCHD_PROJECT_ROOT',
    'PREVIOUS_RUNTIME_DIR',
    'PREVIOUS_COMPILED_RUNTIME_DIR',
    'MISSIOND_BACKUP_RETENTION_DAYS',
    'MISSIOND_BIN_PATH',
    'MISSIOND_MCP_BIN_PATH',
    'MISSIOND_SOCKET_PATH',
    'MISSIOND_LAUNCHCTL_LABEL',
    'MISSIOND_DEPLOY_TIMEOUT',
    'MISSIOND_DEPLOY_SMOKE_TIMEOUT',
    'set -euo pipefail',
    'CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"',
    'cargo build ${BUILD_ARG} -p missiond-daemon -p missiond-mcp',
    'record_timing "cargo-build"',
    'print_timing_summary',
    'RUSTC_WRAPPER',
    'release-manifest.json',
    '"launchd_project_root"',
    '"runtime_dir"',
    '"compiled_runtime_dir"',
    'atomic_symlink_update',
    'switch_active_release',
    'capture_launchd_runtime_state',
    'restart_daemon_supervisor_for_runtime',
    'rollback_to_previous',
    'cleanup_old_releases',
    'create_legacy_release_if_needed',
    'codesign_or_verify "$CANDIDATE_DIR/bin/missiond"',
    'codesign_or_verify "$CANDIDATE_DIR/bin/mission-mcp"',
    'launchctl kickstart -k "gui/$(id -u)/$LABEL"',
    'lsof "$SOCK_PATH"',
    'run_mcp_initialize_smoke()',
    'validate_default_mcp_config',
    'node -e',
    'command -v timeout',
    'command -v gtimeout',
    "perl -e 'alarm shift @ARGV; exec @ARGV'",
    'smoke_start="$(date +%s)"',
    'SMOKE_TIMEOUT="${MISSIOND_DEPLOY_SMOKE_TIMEOUT:-45}"',
    'IPC not ready yet; retrying',
    'active_release=$RELEASE_ID',
    'rollback_with_smoke',
    'rollback-smoke',
    'rollback attempted',
  ]);
  if ((sources.deployDaemon.match(/^\{"mcpServers"/gm) || []).length > 1) {
    diagnostics.push({
      file: files.deployDaemon,
      message: 'default MCP config heredoc must not emit duplicate JSON objects',
    });
  }

  requireAll(diagnostics, files.cargoFmtTouched, sources.cargoFmtTouched, [
    'scripts/cargo-fmt-touched.sh --check',
    'scripts/cargo-fmt-touched.sh --staged',
    'scripts/cargo-fmt-touched.sh --branch main',
    'MODE="all"',
    'CHECK_ONLY=0',
    'git diff --name-only --diff-filter=ACMR',
    'git diff --cached --name-only --diff-filter=ACMR',
    'git ls-files --others --exclude-standard',
    'git diff --name-only --diff-filter=ACMR "${BRANCH}...HEAD"',
    "awk '/\\.rs$/ { print }'",
    'no Rust files in diff',
    'Non-M6 or external-project waves still need a scoped formatter entrypoint',
    'repository-wide MissionD Rust formatting',
    'skip_children=true',
    'command -v rustfmt',
    '--config skip_children=true --check',
    'xargs rustfmt --edition "$EDITION" --config skip_children=true',
  ]);

  requireAll(diagnostics, files.rustfmtMissiond, sources.rustfmtMissiond, [
    'Format/check every Rust file owned by this MissionD repository',
    'scripts/rustfmt-missiond.sh --check',
    'find crates -name',
    'missiond-rustfmt-exempt',
    "grep -E '^[[:space:]]*edition[[:space:]]*=' Cargo.toml",
    'root Cargo.toml has no package edition',
    'edition=$EDITION (from Cargo.toml)',
    'rustfmt --edition "$EDITION" --config skip_children=true --check',
    'rustfmt --edition "$EDITION" --config skip_children=true',
  ]);

  requireAll(diagnostics, files.daemonMain, sources.daemonMain, [
    'env_truthy("MISSIOND_AST_FULL_SYNC_ON_STARTUP")',
    'AST startup full sync skipped',
    'set MISSIOND_AST_FULL_SYNC_ON_STARTUP=1',
  ]);

  requireAll(diagnostics, files.astSyncWorker, sources.astSyncWorker, [
    'topology summary refresh skipped',
    'crate::topology_map::update_module_summaries(store, repo_name).await',
  ]);

  requireAll(diagnostics, files.workspaceCargo, sources.workspaceCargo, [
    "MissionD's runtime database is PostgreSQL-only",
    'the Codex provider-local local-index adapter',
    'rusqlite = { version = "0.31", features = ["bundled"] }',
    'sqlx = { version = "0.8"',
  ]);

  requireAll(diagnostics, files.daemonCargo, sources.daemonCargo, [
    'Read-only Codex CLI provider-local index adapter, not MissionD DB.',
    'rusqlite = { workspace = true }',
  ]);

  requireAll(diagnostics, files.corePgMod, sources.corePgMod, [
    'PostgreSQL backend for MissionD',
    'PostgreSQL is the only MissionD runtime store',
    'provider-local indexes',
    'PgMissionStore',
  ]);

  requireAll(diagnostics, files.coreDbTraits, sources.coreDbTraits, [
    'Domain-specific async traits for MissionD',
    'Provider-local indexes',
    'do not constitute a MissionD database backend',
  ]);

  requireAll(diagnostics, files.codexIngestionWorker, sources.codexIngestionWorker, [
    'Codex Ingestion Worker',
    'state_5.sqlite',
    'SQLITE_OPEN_READ_ONLY',
  ]);

  forbidAll(diagnostics, files.corePgMod, sources.corePgMod, [
    'feature = "sqlite"',
    'migrate_from_sqlite',
  ]);

  forbidAll(diagnostics, files.daemonMain, sources.daemonMain, [
    '--migrate-sqlite-to-pg',
    'migrate_from_sqlite.rs',
  ]);

  forbidAll(diagnostics, files.coreDbTraits, sources.coreDbTraits, [
    'SQLite backend:',
    'SqliteMissionStore',
  ]);

  const legacyMigration = path.join(root, 'crates/missiond-core/src/db/pg/migrate_from_sqlite.rs');
  if (fs.existsSync(legacyMigration)) {
    diagnostics.push({
      file: 'crates/missiond-core/src/db/pg/migrate_from_sqlite.rs',
      message: 'legacy MissionD SQLite migration module must be removed from active code',
    });
  }

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden legacy SQLite contract text still present: ${needle}` });
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
       "Blue-green rollback MUST restore launchd WorkingDirectory, MISSIOND_PROJECT_ROOT, MISSIOND_RUNTIME_DIR, and MISSIOND_COMPILED_RUNTIME_DIR captured before the switch."
       "Release cleanup MUST keep active, previous, and newest retained releases."
       "IPC smoke MUST retry after socket readiness and then rollback on failure."
       "Deploy smoke timeout MUST be configurable through MISSIOND_DEPLOY_SMOKE_TIMEOUT."
       "Deploy scripts MUST emit timing for cargo-build."
       "Dev-only fast deploy may select debug profile and sccache."
       "AST repository-wide startup full sync MUST be opt-in through MISSIOND_AST_FULL_SYNC_ON_STARTUP."
       "M6 MissionD formatting MUST be converged."
       "Rust formatter edition MUST be derived from workspace Cargo.toml."
       "scripts/rustfmt-missiond.sh --check MUST be the MissionD-owned Rust formatter gate."
       "No MissionD Rust source may carry formatter exemption markers."
       "Rust formatting for external or non-M6 projects MAY remain scoped through cargo-fmt-touched."
       "rustfmt MUST run with skip_children=true."
       "MissionD primary runtime database MUST be PostgreSQL-only; the old MissionD SQLite backend, SQLite-to-Postgres migration module, and sqlite feature cfg MUST be absent from active code/build paths."
       "Applied Postgres migration files are immutable, including comments; any schema or documentation change after application MUST use an additive migration or evidence note."
       "SQLite references are allowed only for the external Codex provider-local state_5.sqlite index adapter; skill-store and all MissionD-owned storage MUST use PostgreSQL."])
  (implementation-map
    (surface ops-infra
      :status "code-aligned"
      :code ["scripts/deploy-daemon.sh"
             "scripts/rustfmt-missiond.sh"
             "scripts/cargo-fmt-touched.sh"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
             "crates/missiond-core/src/db/pg/mod.rs"
             "crates/missiond-core/src/db/traits.rs"
             "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
             "scripts/check-v3-ops-infra-isomorphism.mjs"]
      :note "fixture"))
  (compression-contract
    :checks ["node scripts/check-v3-ops-infra-isomorphism.mjs"
             "node scripts/check-pg-migrations-discipline.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.deployDaemon, `
scripts/deploy-daemon.sh                  # build + blue-green deploy + smoke
--build-only --no-smoke --debug --fast --cleanup-only --apply-cleanup
MISSIOND_INSTALL_ROOT MISSIOND_RELEASES_DIR MISSIOND_ACTIVE_LINK MISSIOND_RELEASE_KEEP MISSIOND_BACKUP_RETENTION_DAYS
PREVIOUS_LAUNCHD_PROJECT_ROOT PREVIOUS_RUNTIME_DIR PREVIOUS_COMPILED_RUNTIME_DIR
MISSIOND_BIN_PATH MISSIOND_MCP_BIN_PATH MISSIOND_SOCKET_PATH MISSIOND_LAUNCHCTL_LABEL MISSIOND_DEPLOY_TIMEOUT MISSIOND_DEPLOY_SMOKE_TIMEOUT
MISSIOND_USE_SCCACHE
set -euo pipefail
CARGO_INCREMENTAL="\${CARGO_INCREMENTAL:-0}"
cargo build \${BUILD_ARG} -p missiond-daemon -p missiond-mcp
record_timing "cargo-build"
print_timing_summary
RUSTC_WRAPPER
release-manifest.json
"launchd_project_root"
"runtime_dir"
"compiled_runtime_dir"
atomic_symlink_update
switch_active_release
capture_launchd_runtime_state
restart_daemon_supervisor_for_runtime
rollback_to_previous
cleanup_old_releases
create_legacy_release_if_needed
codesign_or_verify "$CANDIDATE_DIR/bin/missiond"
codesign_or_verify "$CANDIDATE_DIR/bin/mission-mcp"
launchctl kickstart -k "gui/$(id -u)/$LABEL"
lsof "$SOCK_PATH"
run_mcp_initialize_smoke()
validate_default_mcp_config
node -e
command -v timeout
command -v gtimeout
perl -e 'alarm shift @ARGV; exec @ARGV'
smoke_start="$(date +%s)"
SMOKE_TIMEOUT="\${MISSIOND_DEPLOY_SMOKE_TIMEOUT:-45}"
IPC not ready yet; retrying
active_release=$RELEASE_ID
rollback_with_smoke
rollback-smoke
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
git ls-files --others --exclude-standard
git diff --name-only --diff-filter=ACMR "\${BRANCH}...HEAD"
awk '/\\.rs$/ { print }'
no Rust files in diff
Non-M6 or external-project waves still need a scoped formatter entrypoint
repository-wide MissionD Rust formatting
skip_children=true
command -v rustfmt
--config skip_children=true --check
xargs rustfmt --edition "$EDITION" --config skip_children=true
`);

  writeFixture(root, DEFAULT_FILES.rustfmtMissiond, `
Format/check every Rust file owned by this MissionD repository
scripts/rustfmt-missiond.sh --check
find crates -name
missiond-rustfmt-exempt
grep -E '^[[:space:]]*edition[[:space:]]*=' Cargo.toml
root Cargo.toml has no package edition
edition=$EDITION (from Cargo.toml)
rustfmt --edition "$EDITION" --config skip_children=true --check
rustfmt --edition "$EDITION" --config skip_children=true
`);

  writeFixture(root, DEFAULT_FILES.daemonMain, `
env_truthy("MISSIOND_AST_FULL_SYNC_ON_STARTUP")
AST startup full sync skipped
set MISSIOND_AST_FULL_SYNC_ON_STARTUP=1
`);

  writeFixture(root, DEFAULT_FILES.astSyncWorker, `
topology summary refresh skipped
crate::topology_map::update_module_summaries(store, repo_name).await
`);

  writeFixture(root, DEFAULT_FILES.workspaceCargo, `
# MissionD's runtime database is PostgreSQL-only.
# the Codex provider-local local-index adapter
rusqlite = { version = "0.31", features = ["bundled"] }
sqlx = { version = "0.8", features = ["postgres"] }
`);

  writeFixture(root, DEFAULT_FILES.daemonCargo, `
# Read-only Codex CLI provider-local index adapter, not MissionD DB.
rusqlite = { workspace = true }
`);

  writeFixture(root, DEFAULT_FILES.corePgMod, `
//! PostgreSQL backend for MissionD.
//! PostgreSQL is the only MissionD runtime store.
//! provider-local indexes are ingested by workers.
pub struct PgMissionStore;
`);

  writeFixture(root, DEFAULT_FILES.coreDbTraits, `
//! Domain-specific async traits for MissionD's PostgreSQL-backed store.
//! Provider-local indexes do not constitute a MissionD database backend.
`);

  writeFixture(root, DEFAULT_FILES.codexIngestionWorker, `
//! Codex Ingestion Worker
const CODEX_LOCAL_INDEX_RELATIVE: &str = ".codex/state_5.sqlite";
rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
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
