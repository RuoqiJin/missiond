#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { BASELINE_AGENT_ENTRY_IDS } from './lib/v3_agent_slices.mjs';

const AGENT_SLICES = '.missiond/v3/runtime/compiled/compiled-agent-slices.json';
const DEFAULT_CORPUS = '.missiond/v3/evidence/agent-navigation-quality-corpus.json';

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = [];
  const compiled = readJson(AGENT_SLICES, diagnostics);
  const policy = compiled?.payload?.policy ?? {};
  const corpusPath = opts.corpus ?? policy.qualityCorpus ?? DEFAULT_CORPUS;
  const corpus = readJson(corpusPath, diagnostics);
  const entries = Array.isArray(compiled?.payload?.entries) ? compiled.payload.entries : [];
  const fixtures = Array.isArray(corpus?.fixtures) ? corpus.fixtures : [];
  const minFixtures = Number(policy.minFixturesPerEntry ?? 2);
  const minAccuracy = Number(policy.minMatchAccuracy ?? 1);

  for (const id of BASELINE_AGENT_ENTRY_IDS) {
    const count = fixtures.filter((fixture) => fixture?.entryId === id).length;
    if (count < minFixtures) {
      diagnostics.push(diag(corpusPath, `entry ${id} has ${count} fixtures, below required ${minFixtures}`));
    }
  }

  const results = fixtures.map((fixture) => {
    const selected = selectEntry(entries, fixture.intent);
    const selectedId = selected?.id ?? null;
    const ok = selectedId === fixture.entryId;
    if (!ok) {
      diagnostics.push(diag(corpusPath, `intent ${JSON.stringify(fixture.intent)} expected ${fixture.entryId}, got ${selectedId ?? '<none>'}`));
    }
    return { intent: fixture.intent, expected: fixture.entryId, selected: selectedId, ok };
  });
  const accuracy = results.length > 0
    ? results.filter((result) => result.ok).length / results.length
    : 0;
  if (accuracy < minAccuracy) {
    diagnostics.push(diag(corpusPath, `agent navigation match accuracy ${accuracy.toFixed(3)} below required ${minAccuracy}`));
  }

  const payload = {
    ok: diagnostics.length === 0,
    corpus: corpusPath,
    entries: entries.length,
    fixtures: fixtures.length,
    accuracy,
    minFixturesPerEntry: minFixtures,
    minMatchAccuracy: minAccuracy,
    results: opts.verbose ? results : undefined,
    diagnostics,
  };
  if (opts.json) console.log(JSON.stringify(payload, null, 2));
  else if (payload.ok) console.log(`v3 agent navigation quality OK (${fixtures.length} fixtures, accuracy=${accuracy.toFixed(3)})`);
  else for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
  process.exit(payload.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, check: false, verbose: false, corpus: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--check') opts.check = true;
    else if (arg === '--verbose') opts.verbose = true;
    else if (arg === '--corpus') opts.corpus = argv[++i] ?? fail('--corpus requires a value');
    else if (arg.startsWith('--corpus=')) opts.corpus = arg.slice('--corpus='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/check-v3-agent-navigation-quality.mjs [--json] [--check] [--verbose] [--corpus <path>]');
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function selectEntry(entries, intent) {
  const normalized = String(intent ?? '').toLowerCase();
  return entries
    .map((entry) => ({ entry, score: scoreEntry(entry, normalized) }))
    .filter((item) => item.score > 0)
    .sort((a, b) => b.score - a.score || String(a.entry.id).localeCompare(String(b.entry.id)))[0]?.entry ?? null;
}

function scoreEntry(entry, normalizedIntent) {
  let score = 0;
  const id = stringField(entry, 'id');
  if (id && normalizedIntent.includes(id.toLowerCase())) score += 100;
  const label = stringField(entry, 'label');
  if (label) {
    for (const token of label.toLowerCase().split(/[^a-z0-9_-]+/)) {
      if (token.length >= 4 && normalizedIntent.includes(token)) score += 10;
    }
  }
  for (const keyword of stringArray(entry.intentKeywords)) {
    const normalized = keyword.toLowerCase();
    if (normalizedIntent.includes(normalized)) score += 25 + normalized.length;
  }
  for (const surface of stringArray(entry.surfaces)) {
    if (normalizedIntent.includes(surface.toLowerCase())) score += 20;
  }
  return score;
}

function stringField(value, key) {
  return typeof value?.[key] === 'string' ? value[key] : null;
}

function stringArray(value) {
  return Array.isArray(value) ? value.filter((item) => typeof item === 'string') : [];
}

function readJson(rel, diagnostics) {
  try {
    return JSON.parse(fs.readFileSync(path.resolve(rel), 'utf8'));
  } catch (err) {
    diagnostics.push(diag(rel, `cannot read JSON: ${err.message}`));
    return null;
  }
}

function diag(file, message) {
  return { file, line: 1, column: 1, code: 'V3_AGENT_NAVIGATION_QUALITY', message };
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

main();
