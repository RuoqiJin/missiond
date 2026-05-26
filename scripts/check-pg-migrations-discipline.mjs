#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

const usage = `Usage:
  node scripts/check-pg-migrations-discipline.mjs [--json]

Checks MissionD Postgres migration discipline:
- migrations are Postgres-only active contracts
- already-applied frozen migrations keep their exact sqlx checksum
- SQLite/path DB semantics are not introduced
- destructive drops require an explicit missiond-allow-destructive-migration marker
`;

const MIGRATIONS_DIR = 'crates/missiond-core/migrations';
const SQLITE_TOKEN_ALLOW = new Set([
  'crates/missiond-core/migrations/20260525001000_codex_provider_index_state.sql',
]);
const HEADER_ALLOW = new Set([
  'crates/missiond-core/migrations/20260410000000_projects.sql',
  'crates/missiond-core/migrations/20260410300000_project_github_url.sql',
]);
const DESTRUCTIVE_ALLOW = new Set([
  'crates/missiond-core/migrations/20260420200000_drop_system_timeline.sql',
  'crates/missiond-core/migrations/20260421000000_drop_deprecated_tables.sql',
  'crates/missiond-core/migrations/20260523002000_conversation_message_bool_flags.sql',
]);
const FORBIDDEN = [
  ['sqlite token', /\bsqlite\b|\brusqlite\b|\bFTS5\b/i],
  ['MISSION_DB_PATH', /\bMISSION_DB_PATH\b/],
  ['mission.db', /\bmission\.db\b/],
  ['SQLite affinity type', /\bAUTOINCREMENT\b|\bWITHOUT\s+ROWID\b/i],
];

const DESTRUCTIVE = /\bDROP\s+(TABLE|COLUMN|INDEX|SCHEMA)\b|\bTRUNCATE\s+TABLE\b|\bALTER\s+TABLE\b[^;]*\bDROP\b/i;
const ALLOW_MARKER = 'missiond-allow-destructive-migration';
const FROZEN_SHA384 = Object.freeze({
  'crates/missiond-core/migrations/20260318000000_init.sql':
    'cdb48608a9cdfe380c0e00078072195e0a02213e4addecf23643ad40730f8619ed3ceee184e3cc4806a1e8a614e429c9',
  'crates/missiond-core/migrations/20260523003000_codex_replay.sql':
    'e375a6d72a6a9890be7146036b6a060127c345caed5dabae69910a66379295c7ea5860d23a8f42c5eb807a9fd212c924',
  'crates/missiond-core/migrations/20260526000000_conversation_session_management.sql':
    '3f655d3bbc29846f7d83d2c4da4fd3ea4fe0b4633ecfc5dc6d64ab4cd5cdcf6c7a1589eb7139c93d87ca0a538cc40c1c',
});

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
  const dir = path.join(root, MIGRATIONS_DIR);
  const versions = new Map();
  for (const entry of fs.readdirSync(dir).sort()) {
    if (!entry.endsWith('.sql')) continue;
    const rel = path.posix.join(MIGRATIONS_DIR, entry);
    const source = fs.readFileSync(path.join(root, rel), 'utf8');
    checkUniqueVersion(rel, versions, diagnostics);
    checkFrozenChecksum(rel, source, diagnostics);
    checkHeader(rel, source, diagnostics);
    checkForbidden(rel, source, diagnostics);
    checkDestructive(rel, source, diagnostics);
  }

  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('MissionD PG migration discipline check OK');
  } else {
    for (const diagnostic of diagnostics) {
      console.error(`${diagnostic.file}:${diagnostic.line}: ${diagnostic.message}`);
    }
    console.error(`MissionD PG migration discipline check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function checkHeader(file, source, diagnostics) {
  if (HEADER_ALLOW.has(file)) return;
  const first = source.split(/\r?\n/, 1)[0] ?? '';
  if (!first.startsWith('--')) {
    diagnostics.push({ file, line: 1, message: 'migration must start with a SQL comment header' });
  }
}

function checkForbidden(file, source, diagnostics) {
  if (SQLITE_TOKEN_ALLOW.has(file)) return;
  if (frozenChecksumMatches(file, source)) return;
  const lines = source.split(/\r?\n/);
  lines.forEach((line, index) => {
    for (const [name, regex] of FORBIDDEN) {
      if (regex.test(line)) {
        diagnostics.push({ file, line: index + 1, message: `forbidden migration pattern: ${name}` });
      }
    }
  });
}

function frozenChecksumMatches(file, source) {
  const expected = FROZEN_SHA384[file];
  if (!expected) return false;
  return crypto.createHash('sha384').update(source).digest('hex') === expected;
}

function checkDestructive(file, source, diagnostics) {
  if (DESTRUCTIVE_ALLOW.has(file)) return;
  const sql = source
    .split(/\r?\n/)
    .filter((line) => !line.trimStart().startsWith('--'))
    .join('\n');
  if (!DESTRUCTIVE.test(sql)) return;
  if (source.includes(ALLOW_MARKER)) return;
  diagnostics.push({
    file,
    line: 1,
    message: `destructive migration requires -- ${ALLOW_MARKER}: <reason>`,
  });
}

function checkFrozenChecksum(file, source, diagnostics) {
  const expected = FROZEN_SHA384[file];
  if (!expected) return;
  const actual = crypto.createHash('sha384').update(source).digest('hex');
  if (actual === expected) return;
  diagnostics.push({
    file,
    line: 1,
    message: `frozen migration checksum changed: expected sha384 ${expected}, got ${actual}; create an additive migration instead of editing an applied migration`,
  });
}

function checkUniqueVersion(file, versions, diagnostics) {
  const basename = path.posix.basename(file);
  const match = /^(\d+)_/.exec(basename);
  if (!match) {
    diagnostics.push({
      file,
      line: 1,
      message: 'migration filename must start with numeric sqlx version prefix',
    });
    return;
  }
  const version = match[1];
  const existing = versions.get(version);
  if (!existing) {
    versions.set(version, file);
    return;
  }
  diagnostics.push({
    file,
    line: 1,
    message: `duplicate migration version ${version}; already used by ${existing}`,
  });
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
