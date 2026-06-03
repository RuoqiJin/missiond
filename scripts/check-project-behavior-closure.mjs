#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';
import { validateBehaviorClosure } from './lib/behavior_universe.mjs';

const usage = `Usage:
  node scripts/check-project-behavior-closure.mjs --project <id> [--json] [--root <path>] [--repo <path>]

Checks one registered project root for program-level SSOT behavior closure.
MissionD itself is checked against V3 authoring shards; other projects are
checked against project-local .missiond Lisp files.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const root = opts.root ?? resolveProjectRoot(opts.repo, opts.project);
  if (!root) fail(`cannot resolve project root for ${opts.project}`);
  if (!fs.existsSync(root)) {
    const result = {
      ok: false,
      projectId: opts.project,
      root,
      observed_count: 0,
      behavior_count: 0,
      effect_count: 0,
      projection_ok: true,
      projection_diagnostics: [],
      diagnostics: [{
        file: root,
        line: 1,
        column: 1,
        code: 'PROJECT_ROOT_MISSING',
        message: `${opts.project} root does not exist`,
      }],
    };
    emit(result, opts);
  }

  const result = validateBehaviorClosure(root, {
    projectId: opts.project,
    missiondV3: opts.project === 'missiond',
  });
  emit({
    ok: result.ok,
    projectId: opts.project,
    root,
    observed_count: result.observed.length,
    behavior_count: result.declared.behaviors.length,
    effect_count: result.declared.effects.length,
    tombstone_count: result.declared.tombstones.length,
    scanner_profile_count: result.declared.scannerProfiles.length,
    navigation_artifact: result.declared.navigationArtifact,
    projection_ok: result.projection_ok,
    projection_diagnostics: result.projectionDiagnostics,
    diagnostics: result.diagnostics,
  }, opts);
}

function parseArgs(argv) {
  const opts = { json: false, repo: process.cwd(), project: null, root: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--project') opts.project = argv[++i] ?? fail('--project requires a value');
    else if (arg.startsWith('--project=')) opts.project = arg.slice('--project='.length);
    else if (arg === '--root') opts.root = argv[++i] ?? fail('--root requires a value');
    else if (arg.startsWith('--root=')) opts.root = arg.slice('--root='.length);
    else if (arg === '--repo') opts.repo = argv[++i] ?? fail('--repo requires a value');
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!opts.project) fail('--project is required');
  opts.repo = path.resolve(opts.repo);
  if (opts.root) opts.root = path.resolve(opts.root);
  return opts;
}

function resolveProjectRoot(repo, projectId) {
  if (projectId === 'missiond') return repo;
  const registry = path.join(repo, '.missiond/v3/shards/universe/project-registry.lisp');
  if (!fs.existsSync(registry)) return null;
  const forms = parseLisp(fs.readFileSync(registry, 'utf8'), registry);
  for (const project of collectForms(forms, 'project')) {
    const props = readKeywordProps(project, { start: 1 });
    if (nodeText(props[':id']?.value) !== projectId) continue;
    const root = nodeText(props[':root']?.value);
    if (root) return root;
    const relPath = nodeText(props[':path']?.value);
    if (relPath) return repo;
  }
  return null;
}

function collectForms(forms, name) {
  const out = [];
  for (const form of forms) collectFormsInto(form, name, out);
  return out;
}

function collectFormsInto(node, name, out) {
  if (isList(node) && head(node) === name) out.push(node);
  if (!isList(node)) return;
  for (const child of node.children) collectFormsInto(child, name, out);
}

function emit(result, opts) {
  if (opts.json) {
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } else if (result.ok) {
    console.log(`project behavior closure OK (${result.projectId}, ${result.observed_count} observed behaviors)`);
    if (result.projection_ok === false) {
      console.error(`project behavior navigation projection has ${result.projection_diagnostics.length} diagnostic(s)`);
    }
  } else {
    for (const d of result.diagnostics) {
      console.error(`${d.file}:${d.line ?? 1}:${d.column ?? 1}: ${d.code}: ${d.message}`);
    }
    console.error(`project behavior closure FAILED -- ${result.diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function fail(message) {
  console.error(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

main();
