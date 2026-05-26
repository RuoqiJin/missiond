#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-missiond-owned-sqlite-clean.mjs [--json]

Checks active MissionD contracts, code, docs, and Board UI for MissionD-owned
SQLite regressions. The only allowed SQLite usage is the external Codex
provider-local state_5.sqlite adapter, opened read-only.
`;

const ACTIVE_ROOTS = [
  'Cargo.toml',
  'README.md',
  'clippy.toml',
  'crates',
  'docs',
  'packages',
  'scripts',
  '.github',
  '.missiond/v3/shards',
];

const EXCLUDED_DIRS = new Set([
  '.git',
  'target',
  'node_modules',
  '.next',
  'dist',
  'build',
  'coverage',
  '.turbo',
  '.cache',
]);

const EXCLUDED_PREFIXES = [
  'docs/designs/',
  'docs/audit/',
  '.missiond/v3/runtime/',
];

const TEXT_EXTENSIONS = new Set([
  '',
  '.cjs',
  '.css',
  '.html',
  '.js',
  '.json',
  '.lisp',
  '.md',
  '.mjs',
  '.rs',
  '.sh',
  '.sql',
  '.toml',
  '.ts',
  '.tsx',
  '.yaml',
  '.yml',
]);

const ALLOW_SQLITE_TOKEN_FILES = new Set([
  'Cargo.toml',
  'crates/missiond-daemon/Cargo.toml',
  'crates/missiond-core/src/types/conversation.rs',
  'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
  'crates/missiond-daemon/src/startup_preflight.rs',
  'crates/missiond-core/migrations/20260525001000_codex_provider_index_state.sql',
  'scripts/audit-codex-history-ingestion.mjs',
  'scripts/check-v3-ops-infra-isomorphism.mjs',
  'scripts/check-missiond-owned-sqlite-clean.mjs',
  'scripts/check-high-roi-contracts.mjs',
  '.missiond/v3/shards/ops-infra.lisp',
  '.github/workflows/quality-gates.yml',
]);

const ALLOW_STATE_INDEX_FILES = new Set([
  'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
  'crates/missiond-daemon/src/startup_preflight.rs',
  'crates/missiond-core/migrations/20260525001000_codex_provider_index_state.sql',
  'scripts/audit-codex-history-ingestion.mjs',
  'scripts/check-v3-ops-infra-isomorphism.mjs',
  'scripts/check-missiond-owned-sqlite-clean.mjs',
  '.missiond/v3/shards/ops-infra.lisp',
  '.missiond/v3/shards/memory-knowledge-runtime.lisp',
  'docs/architectures/missiond.yaml',
]);

const ALLOW_OLD_SOURCE_STATE_FILES = new Set([
  'crates/missiond-core/src/types/conversation.rs',
  'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
  'crates/missiond-core/migrations/20260525001000_codex_provider_index_state.sql',
  'scripts/check-missiond-owned-sqlite-clean.mjs',
  'scripts/check-high-roi-contracts.mjs',
]);

const FORBIDDEN_PATTERNS = [
  { name: 'MISSION_DB_PATH', regex: /\bMISSION_DB_PATH\b/ },
  { name: 'mission.db runtime path', regex: /\bmission\.db\b/ },
  { name: 'codex_sqlite legacy label', regex: /\bcodex_sqlite\b|\bcodex-sqlite\b/ },
  { name: 'FTS5 legacy label', regex: /\bFTS5\b/ },
  { name: 'SQLite/PostgreSQL dual-engine wording', regex: /SQLite\s*\/\s*PostgreSQL|SQLite\s*\/\s*PG|sqlite\s*\/\s*pg/i },
  { name: 'skill-store SQLite wording', regex: /skill-store[^.\n]*SQLite|SQLite[^.\n]*skill-store/i },
];

function main() {
  const args = process.argv.slice(2);
  const json = args.includes('--json');
  if (args.some((arg) => arg !== '--json' && arg !== '--help' && arg !== '-h')) {
    console.error(usage);
    process.exit(2);
  }
  if (args.includes('--help') || args.includes('-h')) {
    console.log(usage);
    process.exit(0);
  }

  const root = process.cwd();
  const diagnostics = [];
  for (const file of collectFiles(root)) {
    checkFile(root, file, diagnostics);
  }

  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('MissionD-owned SQLite clean check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}: ${d.message}`);
    }
    console.error(`MissionD-owned SQLite clean check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }

  process.exit(result.ok ? 0 : 1);
}

function collectFiles(root) {
  const files = [];
  for (const activeRoot of ACTIVE_ROOTS) {
    const abs = path.join(root, activeRoot);
    if (!fs.existsSync(abs)) continue;
    const stat = fs.statSync(abs);
    if (stat.isFile()) {
      if (isTextFile(activeRoot)) files.push(activeRoot);
    } else {
      walk(root, activeRoot, files);
    }
  }
  return files;
}

function walk(root, relDir, files) {
  if (isExcluded(relDir)) return;
  for (const entry of fs.readdirSync(path.join(root, relDir), { withFileTypes: true })) {
    const rel = path.posix.join(relDir, entry.name);
    if (entry.isDirectory()) {
      if (!EXCLUDED_DIRS.has(entry.name)) walk(root, rel, files);
    } else if (entry.isFile() && isTextFile(rel) && !isExcluded(rel)) {
      files.push(rel);
    }
  }
}

function isTextFile(rel) {
  if (rel.endsWith('Cargo.toml')) return true;
  if (rel.endsWith('clippy.toml')) return true;
  return TEXT_EXTENSIONS.has(path.extname(rel));
}

function isExcluded(rel) {
  return EXCLUDED_PREFIXES.some((prefix) => rel === prefix.slice(0, -1) || rel.startsWith(prefix));
}

function checkFile(root, rel, diagnostics) {
  const source = fs.readFileSync(path.join(root, rel), 'utf8');
  const lines = source.split(/\r?\n/);
  lines.forEach((line, index) => {
    const lineNo = index + 1;

    for (const pattern of FORBIDDEN_PATTERNS) {
      if (pattern.regex.test(line) && !isAllowedForbiddenPattern(rel, line, pattern.name)) {
        diagnostics.push({ file: rel, line: lineNo, message: `forbidden ${pattern.name}` });
      }
    }

    if (/\bsqlite-missing\b/.test(line) && !ALLOW_OLD_SOURCE_STATE_FILES.has(rel)) {
      diagnostics.push({ file: rel, line: lineNo, message: 'legacy source-state label sqlite-missing is only allowed in compatibility migration/reader code' });
    }

    if (/\bstate_5\.sqlite\b/.test(line) && !ALLOW_STATE_INDEX_FILES.has(rel)) {
      diagnostics.push({ file: rel, line: lineNo, message: 'state_5.sqlite may only appear in the Codex provider-local adapter or its active contract/checks' });
    }

    const lineWithoutStateIndex = line.replace(/\bstate_5\.sqlite\b/g, '');
    if (/\b(SQLite|sqlite|rusqlite|sqlite3)\b/.test(lineWithoutStateIndex) && !ALLOW_SQLITE_TOKEN_FILES.has(rel)) {
      diagnostics.push({ file: rel, line: lineNo, message: 'SQLite token outside allowed Codex provider-local boundary' });
    }
  });
}

function isAllowedForbiddenPattern(rel, line, name) {
  if (rel === 'scripts/check-missiond-owned-sqlite-clean.mjs') {
    return true;
  }
  if (name === 'skill-store SQLite wording' && rel === '.missiond/v3/shards/ops-infra.lisp') {
    return line.includes('skill-store and all MissionD-owned storage MUST use PostgreSQL');
  }
  if (name === 'skill-store SQLite wording' && rel === 'scripts/check-v3-ops-infra-isomorphism.mjs') {
    return line.includes('skill-store and all MissionD-owned storage MUST use PostgreSQL');
  }
  if (name === 'skill-store SQLite wording' && rel === '.github/workflows/quality-gates.yml') {
    return true;
  }
  if (name === 'SQLite/PostgreSQL dual-engine wording' && rel === 'scripts/check-v3-ops-infra-isomorphism.mjs') {
    return true;
  }
  if (name === 'mission.db runtime path' && line.includes('MissionControl::db')) {
    return true;
  }
  return false;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
