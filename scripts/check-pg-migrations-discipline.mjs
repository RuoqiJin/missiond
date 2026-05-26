#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-pg-migrations-discipline.mjs [--json]

Checks MissionD Postgres migration discipline:
- migrations are Postgres-only active contracts
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
  for (const entry of fs.readdirSync(dir).sort()) {
    if (!entry.endsWith('.sql')) continue;
    const rel = path.posix.join(MIGRATIONS_DIR, entry);
    const source = fs.readFileSync(path.join(root, rel), 'utf8');
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
  const lines = source.split(/\r?\n/);
  lines.forEach((line, index) => {
    for (const [name, regex] of FORBIDDEN) {
      if (regex.test(line)) {
        diagnostics.push({ file, line: index + 1, message: `forbidden migration pattern: ${name}` });
      }
    }
  });
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

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
