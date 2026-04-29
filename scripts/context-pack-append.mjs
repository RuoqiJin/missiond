#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';
import { validateContextPackSource, ENTRY_HEADS, SCHEMA } from './check-context-pack.mjs';

const usage = `Usage:
  node scripts/context-pack-append.mjs --pack <context-pack.lisp>
    --kind <claim|observation|anchor|shard-proposal|conflict|integration-plan>
    --id <id> --agent <agent>
    [--wave <wave> --purpose <text>] [--summary <text>] [--task <id>]
    [--files <paths>] [--touched <paths>]
    [--shard <name> --owner <worker> --write-scope <paths> --acceptance <commands>]
    [--accepted-shards <names> --dispatch-groups <groups>] [--dispatch-group-shards <group:shard+shard,...>]
    [--shards <names>]
    [--now <iso>] [--json] [--dry-fixture]

Atomically appends one entry to a MissionD context-pack v1 file:
  - creates the pack when missing if --wave and --purpose are supplied
  - allocates the next :seq under a sibling lock
  - injects :at from --now or current UTC time
  - validates candidate bytes with scripts/check-context-pack.mjs before rename

List flags accept comma-separated values.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    runFixtures().then(
      (result) => {
        if (opts.json) console.log(JSON.stringify(result, null, 2));
        else console.log(`context-pack-append fixtures OK (${result.cases} cases)`);
      },
      (err) => {
        console.error(err?.stack ?? err?.message ?? String(err));
        process.exit(1);
      },
    );
    return;
  }
  if (!opts.pack) fail('--pack is required');
  if (!opts.kind) fail('--kind is required');
  if (!opts.id) fail('--id is required');
  if (!opts.agent) fail('--agent is required');
  appendContextPackEntry(opts).then(
    (result) => {
      if (opts.json) console.log(JSON.stringify(result, null, 2));
      else console.log(`context-pack-append OK (${result.entry.kind} seq ${result.entry.seq})`);
    },
    (err) => {
      console.error(`context-pack-append: ${err?.message ?? String(err)}`);
      process.exit(1);
    },
  );
}

function parseArgs(args) {
  const opts = {
    pack: null,
    kind: null,
    id: null,
    agent: null,
    wave: null,
    purpose: null,
    summary: null,
    task: null,
    files: [],
    touched: [],
    shard: null,
    owner: null,
    writeScope: [],
    mustNotTouch: [],
    acceptance: [],
    acceptedShards: [],
    dispatchGroups: [],
    dispatchGroupShards: [],
    shards: [],
    now: null,
    json: false,
    dryFixture: false,
  };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--pack') opts.pack = need(args, ++i, arg);
    else if (arg === '--kind') opts.kind = need(args, ++i, arg);
    else if (arg === '--id') opts.id = need(args, ++i, arg);
    else if (arg === '--agent') opts.agent = need(args, ++i, arg);
    else if (arg === '--wave') opts.wave = need(args, ++i, arg);
    else if (arg === '--purpose') opts.purpose = need(args, ++i, arg);
    else if (arg === '--summary') opts.summary = need(args, ++i, arg);
    else if (arg === '--task') opts.task = need(args, ++i, arg);
    else if (arg === '--files') opts.files.push(...splitList(need(args, ++i, arg)));
    else if (arg === '--touched') opts.touched.push(...splitList(need(args, ++i, arg)));
    else if (arg === '--shard') opts.shard = need(args, ++i, arg);
    else if (arg === '--owner') opts.owner = need(args, ++i, arg);
    else if (arg === '--write-scope') opts.writeScope.push(...splitList(need(args, ++i, arg)));
    else if (arg === '--must-not-touch') opts.mustNotTouch.push(...splitList(need(args, ++i, arg)));
    else if (arg === '--acceptance') opts.acceptance.push(...splitList(need(args, ++i, arg)));
    else if (arg === '--accepted-shards') opts.acceptedShards.push(...splitList(need(args, ++i, arg)));
    else if (arg === '--dispatch-groups') opts.dispatchGroups.push(...splitList(need(args, ++i, arg)));
    else if (arg === '--dispatch-group-shards') opts.dispatchGroupShards.push(...parseDispatchGroupShards(need(args, ++i, arg)));
    else if (arg === '--shards') opts.shards.push(...splitList(need(args, ++i, arg)));
    else if (arg === '--now') opts.now = need(args, ++i, arg);
    else fail(`unknown argument: ${arg}`);
  }
  return opts;
}

export async function appendContextPackEntry(opts) {
  if (!ENTRY_HEADS.has(opts.kind)) throw new Error(`invalid --kind ${JSON.stringify(opts.kind)}`);
  const packPath = path.resolve(process.cwd(), opts.pack);
  fs.mkdirSync(path.dirname(packPath), { recursive: true });
  return withLock(`${packPath}.lock`, async () => {
    const before = fs.existsSync(packPath)
      ? fs.readFileSync(packPath, 'utf8')
      : initialPackSource(opts);
    const nextSeq = nextSequence(before, packPath);
    const at = opts.now ?? new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
    const entry = renderEntry({ ...opts, seq: nextSeq, at });
    const candidate = spliceBeforeFinalParen(updateHeaderSequence(before, nextSeq), `\n\n${entry}\n`);
    const validation = validateContextPackSource(candidate, packPath);
    if (!validation.ok) {
      const messages = validation.diagnostics.map((d) => `${d.line}:${d.column} ${d.message}`).join('\n');
      throw new Error(`candidate context-pack failed validation:\n${messages}`);
    }
    atomicWrite(packPath, candidate);
    return {
      ok: true,
      path: opts.pack,
      entry: { id: opts.id, kind: opts.kind, seq: nextSeq, at },
    };
  });
}

function initialPackSource(opts) {
  if (!opts.wave || !opts.purpose) {
    throw new Error('context-pack does not exist; supply --wave and --purpose to create it');
  }
  return `(context-pack ${opts.wave}-context-pack
  :schema "${SCHEMA}"
  :wave ${opts.wave}
  :purpose ${quote(opts.purpose)}
  :write-model append-only
  :sequence 0)
`;
}

function nextSequence(source, file) {
  const forms = parseLisp(source, file);
  const pack = forms.find((form) => isList(form) && head(form) === 'context-pack');
  if (!pack) return 1;
  const props = readKeywordProps(pack, { start: 2 });
  const headerSeq = Number(nodeText(props[':sequence']?.value) ?? 0);
  let maxEntry = 0;
  for (const child of pack.children) {
    if (!isList(child) || !ENTRY_HEADS.has(head(child))) continue;
    const entryProps = readKeywordProps(child, { start: 1 });
    const seq = Number(nodeText(entryProps[':seq']?.value) ?? 0);
    if (Number.isInteger(seq) && seq > maxEntry) maxEntry = seq;
  }
  return Math.max(headerSeq, maxEntry) + 1;
}

function renderEntry(opts) {
  const fields = [
    `:id ${opts.id}`,
    `:agent ${opts.agent}`,
    `:seq ${opts.seq}`,
    `:at ${quote(opts.at)}`,
  ];
  if (opts.task) fields.push(`:task ${opts.task}`);
  if (opts.summary) fields.push(`:summary ${quote(opts.summary)}`);
  pushVector(fields, ':files', opts.files);
  pushVector(fields, ':touched', opts.touched);
  if (opts.shard) fields.push(`:shard ${opts.shard}`);
  if (opts.owner) fields.push(`:owner ${opts.owner}`);
  pushVector(fields, ':write-scope', opts.writeScope);
  pushVector(fields, ':must-not-touch', opts.mustNotTouch);
  pushVector(fields, ':acceptance', opts.acceptance);
  pushVector(fields, ':accepted-shards', opts.acceptedShards, { atoms: true });
  pushDispatchGroups(fields, opts);
  pushVector(fields, ':shards', opts.shards, { atoms: true });
  return `  (${opts.kind} ${fields.join('\n    ')})`;
}

function pushDispatchGroups(fields, opts) {
  if (opts.dispatchGroupShards?.length > 0) {
    const rendered = opts.dispatchGroupShards
      .map((group) => `(group :id ${group.id} :shards [${group.shards.join(' ')}])`)
      .join(' ');
    fields.push(`:dispatch-groups [${rendered}]`);
    return;
  }
  pushVector(fields, ':dispatch-groups', opts.dispatchGroups, { atoms: true });
}

function pushVector(fields, key, values, { atoms = false } = {}) {
  if (!values || values.length === 0) return;
  const rendered = values.map((value) => (atoms ? value : quote(value))).join(' ');
  fields.push(`${key} [${rendered}]`);
}

function updateHeaderSequence(source, seq) {
  if (!/:sequence\s+\d+/.test(source)) throw new Error('context-pack header missing :sequence');
  return source.replace(/(:sequence\s+)\d+/, `$1${seq}`);
}

function spliceBeforeFinalParen(source, text) {
  const idx = source.lastIndexOf(')');
  if (idx < 0) throw new Error('context-pack missing final close paren');
  return `${source.slice(0, idx).trimEnd()}${text}${source.slice(idx)}`;
}

async function withLock(lockPath, fn) {
  const deadline = Date.now() + 30_000;
  let fd = null;
  while (fd == null) {
    try {
      fd = fs.openSync(lockPath, 'wx');
    } catch (err) {
      if (err.code !== 'EEXIST' || Date.now() >= deadline) throw err;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
  try {
    return await fn();
  } finally {
    fs.closeSync(fd);
    fs.rmSync(lockPath, { force: true });
  }
}

function atomicWrite(file, source) {
  const tmp = `${file}.${process.pid}.${Date.now()}.tmp`;
  fs.writeFileSync(tmp, source);
  fs.renameSync(tmp, file);
}

function splitList(value) {
  return value
    .split(',')
    .map((v) => v.trim())
    .filter(Boolean);
}

function parseDispatchGroupShards(value) {
  return splitList(value).map((spec) => {
    const match = spec.match(/^([^:=]+)[:=](.+)$/);
    if (!match) fail(`--dispatch-group-shards entry must look like A:alpha+beta, got ${JSON.stringify(spec)}`);
    const id = match[1].trim();
    const shards = match[2]
      .split(/[+|]/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (!id || shards.length === 0) {
      fail(`--dispatch-group-shards entry must include a group id and at least one shard, got ${JSON.stringify(spec)}`);
    }
    return { id, shards };
  });
}

function quote(value) {
  return JSON.stringify(String(value));
}

function need(args, i, flag) {
  const value = args[i];
  if (!value) fail(`${flag} requires a value`);
  return value;
}

function fail(message) {
  console.error(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

async function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-context-pack-append-'));
  const pack = path.join(tmp, 'context-pack.lisp');
  const common = {
    pack,
    wave: 'wave99',
    purpose: 'fixture context pack',
    now: '2026-04-29T00:00:00Z',
  };
  const results = [];
  results.push(
    await appendContextPackEntry({
      ...common,
      kind: 'claim',
      id: 'wave99-c1',
      agent: 'agent-a',
      summary: 'claim',
    }),
  );
  results.push(
    await appendContextPackEntry({
      ...common,
      kind: 'shard-proposal',
      id: 'wave99-s2',
      agent: 'agent-a',
      shard: 'alpha',
      owner: 'worker-a',
      writeScope: ['a.rs'],
      mustNotTouch: ['b.rs'],
      acceptance: ['cargo test -p a'],
      now: '2026-04-29T00:00:01Z',
    }),
  );
  results.push(
    await appendContextPackEntry({
      ...common,
      kind: 'integration-plan',
      id: 'wave99-i3',
      agent: 'integrator',
      acceptedShards: ['alpha'],
      dispatchGroupShards: [{ id: 'A', shards: ['alpha'] }],
      now: '2026-04-29T00:00:02Z',
    }),
  );
  const source = fs.readFileSync(pack, 'utf8');
  const validation = validateContextPackSource(source, pack);
  if (!validation.ok) {
    console.error(JSON.stringify(validation.diagnostics, null, 2));
    process.exit(1);
  }
  return { ok: true, cases: results.length };
}

main();
