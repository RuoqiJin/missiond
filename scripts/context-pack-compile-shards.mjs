#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
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
import { validateContextPackSource, ENTRY_HEADS } from './check-context-pack.mjs';

const usage = `Usage:
  node scripts/context-pack-compile-shards.mjs [--json] [--dry-fixture] <context-pack.lisp>

Compiles a MissionD context-pack integration-plan into dispatchable shard
metadata for the code-implementation phase. The context-pack remains the Lisp
SSOT; this script only projects the latest integration-plan into JSON:
  - accepted shard proposals with owner/write_scope/must_not_touch/acceptance
  - dispatch_groups, including mapped groups when the Lisp uses
      :dispatch-groups [(group :id A :shards [alpha beta]) ...]
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

  try {
    if (dryFixture) {
      const result = runFixtures();
      if (json) console.log(JSON.stringify(result, null, 2));
      else console.log(`context-pack shard compiler fixtures OK (${result.cases} cases)`);
      return;
    }
    if (inputs.length !== 1) fail(usage);
    const file = path.resolve(process.cwd(), inputs[0]);
    const source = fs.readFileSync(file, 'utf8');
    const result = compileContextPackSource(source, file);
    if (json) console.log(JSON.stringify(result, null, 2));
    else {
      console.log(
        `context-pack shard compile OK (${result.accepted_shards.length} shard(s), ${result.dispatch_groups.length} dispatch group(s), ${result.group_mode})`,
      );
    }
  } catch (err) {
    console.error(`context-pack-compile-shards: ${err?.message ?? String(err)}`);
    process.exit(1);
  }
}

export function compileContextPackSource(source, file = '<memory>') {
  const validation = validateContextPackSource(source, file);
  if (!validation.ok) {
    const message = validation.diagnostics
      .map((d) => `${d.file}:${d.line}:${d.column}: ${d.message}`)
      .join('\n');
    throw new Error(`context-pack failed validation:\n${message}`);
  }

  const forms = parseLisp(source, file);
  const pack = forms.find((form) => isList(form) && head(form) === 'context-pack');
  if (!pack) throw new Error('no (context-pack ...) form found');

  const packProps = readKeywordProps(pack, { start: 2 });
  const proposals = new Map();
  const integrationPlans = [];

  for (const child of pack.children) {
    if (!isList(child) || !ENTRY_HEADS.has(head(child))) continue;
    const props = readKeywordProps(child, { start: 1 });
    const seq = Number(nodeText(props[':seq']?.value) ?? 0);
    if (head(child) === 'shard-proposal') {
      const shard = keywordPropText(props, ':shard');
      if (!shard) continue;
      proposals.set(shard, {
        shard,
        entry_id: keywordPropText(props, ':id'),
        agent: keywordPropText(props, ':agent'),
        seq,
        owner: keywordPropText(props, ':owner'),
        summary: keywordPropText(props, ':summary') ?? '',
        write_scope: nodeToStringArray(props[':write-scope']?.value),
        must_not_touch: nodeToStringArray(props[':must-not-touch']?.value),
        acceptance: nodeToStringArray(props[':acceptance']?.value),
      });
    } else if (head(child) === 'integration-plan') {
      integrationPlans.push({ node: child, props, seq });
    }
  }

  if (integrationPlans.length === 0) {
    throw new Error('context-pack has no integration-plan entry');
  }
  integrationPlans.sort((a, b) => a.seq - b.seq);
  const latest = integrationPlans[integrationPlans.length - 1];
  const acceptedNames = nodeToStringArray(latest.props[':accepted-shards']?.value);
  const accepted = acceptedNames.map((name) => {
    const proposal = proposals.get(name);
    if (!proposal) throw new Error(`integration-plan accepts unknown shard ${JSON.stringify(name)}`);
    return proposal;
  });
  const dispatchGroups = readDispatchGroups(latest.props[':dispatch-groups']?.value);
  const mapped = dispatchGroups.length > 0 && dispatchGroups.every((g) => g.shards.length > 0);

  return {
    ok: true,
    context_pack: nodeText(pack.children[1]),
    wave: keywordPropText(packProps, ':wave'),
    purpose: keywordPropText(packProps, ':purpose'),
    integration_plan: {
      entry_id: keywordPropText(latest.props, ':id'),
      seq: latest.seq,
      summary: keywordPropText(latest.props, ':summary') ?? '',
    },
    accepted_shards: accepted,
    dispatch_groups: dispatchGroups,
    group_mode: mapped ? 'mapped' : 'names_only',
    dispatchable_groups: mapped
      ? dispatchGroups.map((group) => ({
          id: group.id,
          shards: group.shards.map((name) => proposals.get(name)),
        }))
      : [],
  };
}

function readDispatchGroups(node) {
  if (!node || !isList(node)) return [];
  const out = [];
  for (const child of node.children) {
    const text = nodeText(child);
    if (text != null && text !== '') {
      out.push({ id: text, shards: [] });
      continue;
    }
    if (isList(child) && head(child) === 'group') {
      const props = readKeywordProps(child, { start: 1 });
      out.push({
        id: keywordPropText(props, ':id'),
        shards: nodeToStringArray(props[':shards']?.value),
      });
    }
  }
  return out;
}

function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-context-pack-compile-'));
  const cases = [
    {
      name: 'mapped dispatch groups compile to dispatchable shards',
      source: `(context-pack wave99-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave99
  :purpose "compile mapped groups"
  :write-model append-only
  :sequence 3
  (shard-proposal :id s1 :agent context-a :seq 1 :at "2026-04-29T00:00:00Z" :shard alpha :owner worker-a :summary "alpha shard" :write-scope ["a.rs"] :must-not-touch ["b.rs"] :acceptance ["cargo test -p a"])
  (shard-proposal :id s2 :agent context-b :seq 2 :at "2026-04-29T00:00:01Z" :shard beta :owner worker-b :summary "beta shard" :write-scope ["b.rs"] :must-not-touch ["a.rs"] :acceptance ["cargo test -p b"])
  (integration-plan :id i3 :agent integrator :seq 3 :at "2026-04-29T00:00:02Z" :summary "accept both" :accepted-shards [alpha beta] :dispatch-groups [(group :id A :shards [alpha]) (group :id B :shards [beta])]))`,
      assert(result) {
        if (result.group_mode !== 'mapped') throw new Error('expected mapped group mode');
        if (result.dispatchable_groups.length !== 2) throw new Error('expected two dispatchable groups');
        if (result.dispatchable_groups[0].shards[0].write_scope[0] !== 'a.rs') {
          throw new Error('expected shard metadata in dispatchable group');
        }
      },
    },
    {
      name: 'legacy group ids remain names-only',
      source: `(context-pack wave99-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave99
  :purpose "compile names-only groups"
  :write-model append-only
  :sequence 2
  (shard-proposal :id s1 :agent context-a :seq 1 :at "2026-04-29T00:00:00Z" :shard alpha :owner worker-a :summary "alpha shard" :write-scope ["a.rs"] :must-not-touch ["b.rs"] :acceptance ["true"])
  (integration-plan :id i2 :agent integrator :seq 2 :at "2026-04-29T00:00:01Z" :accepted-shards [alpha] :dispatch-groups [A]))`,
      assert(result) {
        if (result.group_mode !== 'names_only') throw new Error('expected names_only group mode');
        if (result.accepted_shards.length !== 1) throw new Error('expected accepted shard metadata');
      },
    },
  ];

  for (const c of cases) {
    const file = path.join(tmp, `${c.name.replace(/[^a-z0-9]+/gi, '-')}.lisp`);
    const result = compileContextPackSource(c.source, file);
    c.assert(result);
  }
  return { ok: true, cases: cases.length };
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
