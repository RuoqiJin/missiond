#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import {
  head,
  isList,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';

export const SCHEMA = 'missiond.context-pack.v1';
export const CONTEXT_PACK_HEAD = 'context-pack';
export const ENTRY_HEADS = new Set([
  'claim',
  'observation',
  'anchor',
  'shard-proposal',
  'conflict',
  'integration-plan',
]);

const ID_RE = /^[a-z0-9][a-z0-9._-]*$/;
const ISO8601_RE =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})$/;

const usage = `Usage:
  node scripts/check-context-pack.mjs [--json] [--dry-fixture] <context-pack.lisp...>

Checks MissionD context-pack v1 Lisp records:
  - validates a single (context-pack ...) append-only multi-writer context ledger
  - enforces header :schema, :wave, :purpose, :write-model append-only, :sequence
  - validates entry heads: claim, observation, anchor, shard-proposal, conflict, integration-plan
  - enforces unique :id, strictly increasing :seq, ISO timestamps, and repo-relative paths
  - validates shard proposals and the final integration-plan references accepted shard names

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  const inputs = [];

  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      inputs.push(arg);
    }
  }

  if (dryFixture) {
    const result = runFixtures();
    if (json) console.log(JSON.stringify(result, null, 2));
    else console.log(`context-pack fixtures OK (${result.cases} cases)`);
    return;
  }

  if (inputs.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  const result = validateContextPackFiles(
    inputs.map((input) => path.resolve(process.cwd(), input)),
  );
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(
      `context-pack check OK (${result.packs} pack${result.packs === 1 ? '' : 's'}, ${result.entries} entries)`,
    );
  } else {
    for (const d of result.diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `context-pack check FAILED -- ${result.diagnostics.length} diagnostic(s), ${result.packs} pack(s), ${result.entries} entries`,
    );
  }
  process.exit(result.ok ? 0 : 1);
}

export function validateContextPackFiles(files) {
  const diagnostics = [];
  let packs = 0;
  let entries = 0;
  for (const file of files) {
    let source;
    try {
      source = fs.readFileSync(file, 'utf8');
    } catch (err) {
      diagnostics.push(error(file, { line: 1, column: 1 }, `cannot read: ${err.message}`));
      continue;
    }
    const result = validateContextPackSource(source, file);
    packs += result.packs;
    entries += result.entries;
    diagnostics.push(...result.diagnostics);
  }
  return {
    ok: !diagnostics.some((d) => d.severity === 'error'),
    files: files.length,
    packs,
    entries,
    diagnostics,
  };
}

export function validateContextPackSource(source, file = '<memory>') {
  const diagnostics = [];
  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    diagnostics.push(error(file, err, err.message));
    return { ok: false, packs: 0, entries: 0, diagnostics };
  }

  const packs = forms.filter((form) => isList(form) && head(form) === CONTEXT_PACK_HEAD);
  if (packs.length === 0) {
    diagnostics.push(error(file, { line: 1, column: 1 }, 'no (context-pack ...) form found'));
    return { ok: false, packs: 0, entries: 0, diagnostics };
  }
  if (packs.length > 1) {
    diagnostics.push(
      error(file, packs[1].loc, 'expected exactly one (context-pack ...) form per file'),
    );
  }

  let entries = 0;
  for (const pack of packs) {
    entries += validatePack(file, pack, diagnostics);
  }
  return {
    ok: !diagnostics.some((d) => d.severity === 'error'),
    packs: packs.length,
    entries,
    diagnostics,
  };
}

function validatePack(file, pack, diagnostics) {
  const props = readKeywordProps(pack, { start: 2 });
  const id = nodeText(pack.children[1]);
  if (!id || !ID_RE.test(id)) addError(diagnostics, file, pack.loc, 'context-pack id must be a lowercase token');

  requireText(diagnostics, file, props, ':schema', SCHEMA);
  requirePresent(diagnostics, file, props, ':wave');
  requirePresent(diagnostics, file, props, ':purpose');
  requireText(diagnostics, file, props, ':write-model', 'append-only');
  const declaredSequence = requireInteger(diagnostics, file, props, ':sequence');

  const entries = pack.children.filter((child) => isList(child) && ENTRY_HEADS.has(head(child)));
  if (entries.length === 0) addError(diagnostics, file, pack.loc, 'context-pack must contain at least one append-only entry');

  const seenIds = new Map();
  const seenSeqs = new Map();
  const shardScopes = new Map();
  const shardLocs = new Map();
  let lastSeq = 0;

  for (const entry of entries) {
    const entryHead = head(entry);
    const entryProps = readKeywordProps(entry, { start: 1 });
    const entryId = requirePresent(diagnostics, file, entryProps, ':id');
    const seq = requireInteger(diagnostics, file, entryProps, ':seq');
    requirePresent(diagnostics, file, entryProps, ':agent');
    const at = requirePresent(diagnostics, file, entryProps, ':at');
    if (at && !ISO8601_RE.test(at)) addError(diagnostics, file, entryProps[':at'].value.loc, ':at must be an ISO-8601 timestamp');

    if (entryId) {
      if (!ID_RE.test(entryId)) addError(diagnostics, file, entryProps[':id'].value.loc, ':id must be a lowercase token');
      if (seenIds.has(entryId)) {
        addError(diagnostics, file, entryProps[':id'].value.loc, `duplicate entry :id "${entryId}" (also at seq ${seenIds.get(entryId)})`);
      } else {
        seenIds.set(entryId, seq ?? '?');
      }
    }
    if (Number.isInteger(seq)) {
      if (seq <= lastSeq) addError(diagnostics, file, entryProps[':seq'].value.loc, `:seq ${seq} must be greater than previous :seq ${lastSeq}`);
      if (seenSeqs.has(seq)) addError(diagnostics, file, entryProps[':seq'].value.loc, `duplicate :seq ${seq}`);
      seenSeqs.set(seq, true);
      lastSeq = seq;
    }

    validateRepoPaths(diagnostics, file, entryProps, ':files');
    validateRepoPaths(diagnostics, file, entryProps, ':touched');
    validateRepoPaths(diagnostics, file, entryProps, ':write-scope');
    validateRepoPaths(diagnostics, file, entryProps, ':must-not-touch');

    if (entryHead === 'claim' || entryHead === 'observation') {
      requirePresent(diagnostics, file, entryProps, ':summary');
    } else if (entryHead === 'anchor') {
      if (!entryProps[':files'] && !entryProps[':touched']) {
        addError(diagnostics, file, entry.loc, 'anchor requires :files or :touched');
      }
      requirePresent(diagnostics, file, entryProps, ':summary');
    } else if (entryHead === 'shard-proposal') {
      const shard = requirePresent(diagnostics, file, entryProps, ':shard');
      requirePresent(diagnostics, file, entryProps, ':owner');
      requireNonEmptyVector(diagnostics, file, entryProps, ':write-scope');
      requireNonEmptyVector(diagnostics, file, entryProps, ':acceptance');
      if (shard) {
        const scope = nodeToStringArray(entryProps[':write-scope']?.value).map(normalizeRepoPath);
        shardScopes.set(shard, scope);
        shardLocs.set(shard, entry.loc);
      }
    } else if (entryHead === 'conflict') {
      requirePresent(diagnostics, file, entryProps, ':summary');
      if (!entryProps[':files'] && !entryProps[':shards']) {
        addError(diagnostics, file, entry.loc, 'conflict requires :files or :shards');
      }
    } else if (entryHead === 'integration-plan') {
      const accepted = requireNonEmptyVector(diagnostics, file, entryProps, ':accepted-shards');
      requireNonEmptyVector(diagnostics, file, entryProps, ':dispatch-groups');
      validateAcceptedShards(diagnostics, file, accepted, entryProps[':accepted-shards']?.value?.loc, shardScopes, shardLocs);
    }
  }

  if (Number.isInteger(declaredSequence) && declaredSequence < lastSeq) {
    addError(diagnostics, file, props[':sequence'].value.loc, `header :sequence ${declaredSequence} must be >= max entry :seq ${lastSeq}`);
  }

  for (const child of pack.children.slice(2)) {
    if (!isList(child)) continue;
    const childHead = head(child);
    if (childHead !== CONTEXT_PACK_HEAD && !ENTRY_HEADS.has(childHead)) {
      addError(diagnostics, file, child.loc, `unknown context-pack child head "${childHead}"`);
    }
  }

  return entries.length;
}

function validateAcceptedShards(diagnostics, file, accepted, loc, shardScopes, shardLocs) {
  for (const shard of accepted) {
    if (!shardScopes.has(shard)) {
      addError(diagnostics, file, loc, `integration-plan accepts unknown shard "${shard}"`);
    }
  }
  for (let i = 0; i < accepted.length; i += 1) {
    for (let j = i + 1; j < accepted.length; j += 1) {
      const a = accepted[i];
      const b = accepted[j];
      const overlap = firstScopeOverlap(shardScopes.get(a) ?? [], shardScopes.get(b) ?? []);
      if (overlap) {
        addError(
          diagnostics,
          file,
          shardLocs.get(b) ?? loc,
          `accepted shards "${a}" and "${b}" overlap on write-scope "${overlap}"`,
        );
      }
    }
  }
}

function firstScopeOverlap(left, right) {
  for (const a of left) {
    for (const b of right) {
      if (a === b) return a;
      if (!hasGlob(a) && !hasGlob(b)) {
        const aa = a.endsWith('/') ? a : `${a}/`;
        const bb = b.endsWith('/') ? b : `${b}/`;
        if (aa.startsWith(bb) || bb.startsWith(aa)) return aa.startsWith(bb) ? b : a;
      }
    }
  }
  return null;
}

function hasGlob(value) {
  return /[*?[\]]/.test(value);
}

function requireText(diagnostics, file, props, key, expected) {
  const actual = requirePresent(diagnostics, file, props, key);
  if (actual != null && actual !== expected) {
    addError(diagnostics, file, props[key].value.loc, `${key} must be "${expected}", got ${JSON.stringify(actual)}`);
  }
  return actual;
}

function requirePresent(diagnostics, file, props, key) {
  const value = nodeText(props[key]?.value);
  if (value == null || value === '') {
    addError(diagnostics, file, props[key]?.keyNode?.loc ?? { line: 1, column: 1 }, `missing required ${key}`);
    return null;
  }
  return value;
}

function requireInteger(diagnostics, file, props, key) {
  const raw = requirePresent(diagnostics, file, props, key);
  if (raw == null) return null;
  const value = Number(raw);
  if (!Number.isInteger(value) || value < 0) {
    addError(diagnostics, file, props[key].value.loc, `${key} must be a non-negative integer`);
    return null;
  }
  return value;
}

function requireNonEmptyVector(diagnostics, file, props, key) {
  const values = nodeToStringArray(props[key]?.value);
  if (values.length === 0) addError(diagnostics, file, props[key]?.keyNode?.loc ?? { line: 1, column: 1 }, `${key} must be a non-empty vector`);
  return values;
}

function validateRepoPaths(diagnostics, file, props, key) {
  if (!props[key]) return;
  const values = nodeToStringArray(props[key].value);
  if (values.length === 0) {
    addError(diagnostics, file, props[key].value.loc, `${key} must be a non-empty vector`);
    return;
  }
  for (const value of values) {
    const normalized = normalizeRepoPath(value);
    if (!normalized || path.isAbsolute(value) || normalized.startsWith('../') || normalized.includes('/../') || normalized === '..') {
      addError(diagnostics, file, props[key].value.loc, `${key} path must be repo-relative: ${JSON.stringify(value)}`);
    }
  }
}

function normalizeRepoPath(value) {
  return String(value ?? '').replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
}

function addError(diagnostics, file, loc, message) {
  diagnostics.push(error(file, loc, message));
}

function error(file, loc, message) {
  return {
    severity: 'error',
    file,
    line: loc?.line ?? 1,
    column: loc?.column ?? 1,
    message,
  };
}

function runFixtures() {
  const cases = [
    {
      name: 'pass: parallel observations + shard proposals + integration plan',
      ok: true,
      source: `(context-pack wave99-context
  :schema "missiond.context-pack.v1"
  :wave wave99
  :purpose "Parallel investigation produces append-only context and shards."
  :write-model append-only
  :sequence 6
  (claim :id wave99-c1 :agent context-a :seq 1 :at "2026-04-29T00:00:00Z" :summary "claim")
  (observation :id wave99-o2 :agent context-a :seq 2 :at "2026-04-29T00:00:01Z" :files ["a.rs"] :summary "finding")
  (anchor :id wave99-a3 :agent context-b :seq 3 :at "2026-04-29T00:00:02Z" :files ["b.rs"] :summary "anchor")
  (shard-proposal :id wave99-s4 :agent context-a :seq 4 :at "2026-04-29T00:00:03Z" :shard rust-a :owner worker-a :write-scope ["a.rs"] :acceptance ["cargo test -p x"])
  (shard-proposal :id wave99-s5 :agent context-b :seq 5 :at "2026-04-29T00:00:04Z" :shard rust-b :owner worker-b :write-scope ["b.rs"] :acceptance ["cargo test -p y"])
  (integration-plan :id wave99-i6 :agent integrator :seq 6 :at "2026-04-29T00:00:05Z" :accepted-shards [rust-a rust-b] :dispatch-groups [A]))`,
    },
    {
      name: 'fail: duplicate seq rejected',
      ok: false,
      expect: /duplicate :seq|greater than previous/,
      source: `(context-pack bad
  :schema "missiond.context-pack.v1" :wave wave99 :purpose "bad" :write-model append-only :sequence 2
  (claim :id a :agent x :seq 1 :at "2026-04-29T00:00:00Z" :summary "a")
  (observation :id b :agent y :seq 1 :at "2026-04-29T00:00:01Z" :summary "b"))`,
    },
    {
      name: 'fail: accepted shard must exist',
      ok: false,
      expect: /unknown shard/,
      source: `(context-pack bad
  :schema "missiond.context-pack.v1" :wave wave99 :purpose "bad" :write-model append-only :sequence 1
  (integration-plan :id i :agent z :seq 1 :at "2026-04-29T00:00:00Z" :accepted-shards [missing] :dispatch-groups [A]))`,
    },
    {
      name: 'fail: accepted shard write scopes cannot overlap',
      ok: false,
      expect: /overlap/,
      source: `(context-pack bad
  :schema "missiond.context-pack.v1" :wave wave99 :purpose "bad" :write-model append-only :sequence 3
  (shard-proposal :id s1 :agent a :seq 1 :at "2026-04-29T00:00:00Z" :shard one :owner w1 :write-scope ["crates/x"] :acceptance ["true"])
  (shard-proposal :id s2 :agent b :seq 2 :at "2026-04-29T00:00:01Z" :shard two :owner w2 :write-scope ["crates/x/src/lib.rs"] :acceptance ["true"])
  (integration-plan :id i :agent z :seq 3 :at "2026-04-29T00:00:02Z" :accepted-shards [one two] :dispatch-groups [A]))`,
    },
  ];

  let failed = 0;
  for (const c of cases) {
    const result = validateContextPackSource(c.source, `<fixture:${c.name}>`);
    const matches = c.expect ? c.expect.test(result.diagnostics.map((d) => d.message).join('\n')) : true;
    if (result.ok !== c.ok || !matches) {
      failed += 1;
      console.error(`FAIL ${c.name}`);
      console.error(JSON.stringify(result.diagnostics, null, 2));
    }
  }
  if (failed > 0) {
    console.error(`context-pack fixtures FAILED -- ${failed} of ${cases.length}`);
    process.exit(1);
  }
  return { ok: true, cases: cases.length };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
