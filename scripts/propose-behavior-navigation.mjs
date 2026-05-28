#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import os from 'node:os';

import {
  loadDeclaredBehaviorUniverse,
  scanObservedUniverse,
} from './lib/behavior_universe.mjs';

const NAVIGATION_RISK_KINDS = new Set([
  'worker',
  'scheduler',
  'background-task',
  'mcp-tool',
  'route',
  'cli',
  'subprocess',
]);

const usage = `Usage:
  node scripts/propose-behavior-navigation.mjs --project <id> [--json] [--write] [--root <path>] [--repo <path>]

Generates deterministic navigation anchors from scanner output. For MissionD V3,
the generated anchors are written as a compiled runtime artifact, not active
authoring Lisp.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const root = opts.root ?? resolveProjectRoot(opts.repo, opts.project);
  if (!root) fail(`cannot resolve project root for ${opts.project}`);
  if (!fs.existsSync(root)) fail(`project root does not exist: ${root}`);

  const missiondV3 = opts.project === 'missiond';
  const observed = scanObservedUniverse(root, { projectId: opts.project });
  const declared = loadDeclaredBehaviorUniverse(root, { projectId: opts.project, missiondV3 });
  const riskItems = observed.filter(isNavigationRisk);
  const navigationForms = generateNavigationForms(opts.project, riskItems);
  const target = behaviorUniverseTarget(root, { missiondV3 });
  const artifact = missiondV3
    ? compiledBehaviorNavigationArtifact({
        projectId: opts.project,
        root,
        target,
        observed,
        riskItems,
        navigationForms,
      })
    : null;
  const result = {
    ok: true,
    projectId: opts.project,
    root,
    target,
    observed_count: observed.length,
    risk_count: riskItems.length,
    anchor_count: riskItems.length,
    effect_anchor_count: riskItems.filter((item) => item.kind === 'effect').length,
    declared_effect_count: declared.effects.length,
    forms: navigationForms.trimEnd(),
    artifact,
  };

  if (opts.write) {
    if (missiondV3) {
      writeCompiledBehaviorNavigation(target, artifact);
      stripMissiondV3NavigationBlock(root);
    } else {
      writeNavigationForms(target, opts.project, root, navigationForms);
    }
    result.written = true;
  } else {
    result.written = false;
  }

  if (opts.json) {
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } else {
    console.log(`${opts.project}: proposed ${result.anchor_count} navigation anchor(s) for ${target}`);
    if (opts.write) console.log(`${opts.project}: wrote navigation anchors`);
  }
}

function parseArgs(argv) {
  const opts = {
    json: false,
    write: false,
    repo: process.cwd(),
    root: null,
    project: null,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--write') opts.write = true;
    else if (arg === '--project') opts.project = argv[++i] ?? fail('--project requires a value');
    else if (arg.startsWith('--project=')) opts.project = arg.slice('--project='.length);
    else if (arg === '--root') opts.root = path.resolve(argv[++i] ?? fail('--root requires a value'));
    else if (arg.startsWith('--root=')) opts.root = path.resolve(arg.slice('--root='.length));
    else if (arg === '--repo') opts.repo = path.resolve(argv[++i] ?? fail('--repo requires a value'));
    else if (arg.startsWith('--repo=')) opts.repo = path.resolve(arg.slice('--repo='.length));
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!opts.project) fail('--project is required');
  opts.repo = path.resolve(opts.repo);
  return opts;
}

function resolveProjectRoot(repo, projectId) {
  if (projectId === 'missiond') return repo;
  const runtimeDir = process.env.MISSIOND_RUNTIME_DIR
    || path.join(os.homedir(), '.missiond/runtime/missiond');
  const compiled = [
    path.join(runtimeDir, 'compiled/compiled-project-universe.json'),
    path.join(repo, '.missiond/v3/runtime/compiled/compiled-project-universe.json'),
  ].find((candidate) => fs.existsSync(candidate));
  if (!compiled) return null;
  const payload = JSON.parse(fs.readFileSync(compiled, 'utf8')).payload ?? {};
  const project = (payload.projects ?? []).find((entry) => entry.id === projectId);
  return project?.root ?? null;
}

function behaviorUniverseTarget(root, { missiondV3 }) {
  if (missiondV3) {
    const runtimeDir = process.env.MISSIOND_RUNTIME_DIR
      || path.join(os.homedir(), '.missiond/runtime/missiond');
    return path.join(runtimeDir, 'compiled/compiled-behavior-navigation.json');
  }
  return path.join(root, '.missiond/behavior-universe.lisp');
}

function isNavigationRisk(item) {
  return NAVIGATION_RISK_KINDS.has(item.kind)
    || (item.kind === 'effect' && item.scope === 'external-home')
    || (item.kind === 'effect' && Boolean(item.effectHint));
}

function generateNavigationForms(projectId, items) {
  const groups = new Map();
  for (const item of items) {
    const kind = behaviorKindFor(item);
    if (!groups.has(kind)) groups.set(kind, []);
    groups.get(kind).push(item);
  }

  const forms = [];
  for (const [kind, group] of [...groups.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    const id = `${projectId}-navigation-${kind}`;
    const observed = unique(group.map((item) => navigationObservedId(item)));
    const code = unique(group.map((item) => item.file));
    const effects = unique(group.map((item) => item.effectHint).filter(Boolean));
    const anchors = group.map((item) => formatAnchor(item)).join('\n');
    forms.push(`  (behavior
    :id ${id}
    :kind ${kind}
    :owner navigation-gate
    :observed ${formatArray(observed, 14)}
    :code ${formatArray(code, 10)}
    :effects ${formatSymbolArray(effects, 13)}
${anchors})`);
  }

  if (forms.length === 0) return '';
  return `  ;; BEGIN GENERATED NAVIGATION ANCHORS
${forms.join('\n\n')}
  ;; END GENERATED NAVIGATION ANCHORS
`;
}

function behaviorKindFor(item) {
  if (item.kind === 'background-task') return 'scheduler';
  return item.kind;
}

function formatAnchor(item) {
  const role = item.role ?? roleForKind(item.kind);
  const parts = [
    `      :role ${role}`,
    `      :observed ${quote(navigationObservedId(item))}`,
    `      :file ${quote(item.file)}`,
  ];
  if (item.symbol) parts.push(`      :symbol ${quote(item.symbol)}`);
  if (item.effectHint) parts.push(`      :effect ${item.effectHint}`);
  return `    (anchor
${parts.join('\n')})`;
}

function navigationObservedId(item) {
  if (!item.symbol) return item.id;
  const suffix = `:${item.file}:${item.line}`;
  if (!item.id.endsWith(suffix)) return item.id;
  const prefix = item.id.slice(0, -suffix.length);
  return `${prefix}:${item.file}:*`;
}

function roleForKind(kind) {
  if (kind === 'mcp-tool') return 'tool';
  if (kind === 'background-task') return 'scheduler';
  if (kind === 'effect') return 'effect-site';
  if (kind === 'cli') return 'entry';
  return kind;
}

function writeNavigationForms(target, projectId, root, navigationForms) {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  const text = fs.existsSync(target)
    ? fs.readFileSync(target, 'utf8')
    : defaultBehaviorUniverse(projectId);
  const nextText = rewriteOrInsertProjectOverlay(text, projectId, navigationForms);
  fs.writeFileSync(target, nextText, 'utf8');
}

function writeCompiledBehaviorNavigation(target, artifact) {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, `${JSON.stringify(artifact, null, 2)}\n`, 'utf8');
}

function stripMissiondV3NavigationBlock(root) {
  const target = path.join(root, '.missiond/v3/shards/universe/behavior-closure.lisp');
  if (!fs.existsSync(target)) return;
  const text = fs.readFileSync(target, 'utf8');
  const nextText = insertOrReplaceNavigationBlock(text, '');
  if (nextText !== text) fs.writeFileSync(target, nextText, 'utf8');
}

function rewriteOrInsertProjectOverlay(text, projectId, navigationForms) {
  if (text.includes(':status generated-overlay')) {
    return defaultBehaviorUniverse(projectId, navigationForms);
  }
  return insertOrReplaceNavigationBlock(text, navigationForms);
}

function insertOrReplaceNavigationBlock(text, navigationForms) {
  const markerRe = /\n?\s*;; BEGIN GENERATED NAVIGATION ANCHORS[\s\S]*?;; END GENERATED NAVIGATION ANCHORS\n?/;
  let nextText = text.replace(markerRe, '\n');
  if (!navigationForms.trim()) return nextText;
  const effectIndex = nextText.indexOf('\n  (effect\n');
  if (effectIndex >= 0) {
    return `${nextText.slice(0, effectIndex)}\n${navigationForms}${nextText.slice(effectIndex)}`;
  }
  const closeIndex = nextText.lastIndexOf(')');
  if (closeIndex < 0) return `${nextText.trimEnd()}\n${navigationForms}`;
  return `${nextText.slice(0, closeIndex).trimEnd()}\n\n${navigationForms}${nextText.slice(closeIndex)}`;
}

function compiledBehaviorNavigationArtifact({
  projectId,
  root,
  target,
  observed,
  riskItems,
  navigationForms,
}) {
  const anchors = riskItems.map((item) => ({
    id: navigationObservedId(item),
    observed_id: item.id,
    kind: item.kind,
    role: item.role ?? roleForKind(item.kind),
    file: item.file,
    line: item.line ?? 1,
    symbol: item.symbol ?? null,
    effect: item.effectHint ?? null,
  }));
  const payload = {
    projectId,
    root,
    target,
    observed_count: observed.length,
    risk_count: riskItems.length,
    anchor_count: anchors.length,
    anchors,
    forms: navigationForms.trimEnd(),
  };
  return {
    schema_version: 'missiond.compiled-behavior-navigation.v1',
    source_hash: crypto.createHash('md5').update(JSON.stringify(payload)).digest('hex'),
    generated_at: null,
    diagnostics: [],
    payload,
  };
}

function defaultBehaviorUniverse(projectId, navigationForms = '') {
  return `(behavior-universe ${projectId}
  :schema "missiond.behavior-universe.v1"
  :project ${projectId}
  :status generated-overlay
  :owner missiond-project-ssot-convergence
  :rule "Program-level behavior closure overlay generated by MissionD. Observed behavior is scanner evidence; this Lisp file is the editable SSOT claim surface."

  (behavior
    :id ${projectId}-workers-and-schedulers
    :kind worker
    :owner project-runtime
    :observed ["worker:*" "background-task:*" "scheduler:*"]
    :code ["**/*.rs" "**/*.js" "**/*.mjs" "**/*.ts" "**/*.tsx" "**/*.py"]
    :effects [])

  (behavior
    :id ${projectId}-interfaces
    :kind route
    :owner project-runtime
    :observed ["route:*" "cli:*" "mcp-tool:*"]
    :code ["**/*.rs" "**/*.js" "**/*.mjs" "**/*.ts" "**/*.tsx" "**/*.py"]
    :effects [])

  (behavior
    :id ${projectId}-persistence
    :kind db-write
    :owner project-runtime
    :observed ["db-write:*"]
    :code ["**/*.rs" "**/*.js" "**/*.mjs" "**/*.ts" "**/*.tsx" "**/*.py"]
    :effects [])

  (behavior
    :id ${projectId}-process-network-model-io
    :kind subprocess
    :owner project-runtime
    :observed ["subprocess:*" "network:*" "model-call:*"]
    :code ["**/*.rs" "**/*.js" "**/*.mjs" "**/*.ts" "**/*.tsx" "**/*.py"]
    :effects [])

  (behavior
    :id ${projectId}-filesystem-effects
    :kind effect
    :owner project-runtime
    :observed ["effect:fs-write:*" "effect:fs-append:*" "effect:fs-rename:*" "effect:fs-delete:*"]
    :code ["**/*.rs" "**/*.js" "**/*.mjs" "**/*.ts" "**/*.tsx" "**/*.py"]
    :effects [${projectId}-repo-file-write
              ${projectId}-repo-file-append
              ${projectId}-repo-file-rename
              ${projectId}-repo-file-delete])

${navigationForms}  (effect
    :id ${projectId}-repo-file-write
    :feature ${projectId}-runtime-artifacts
    :kind filesystem-write
    :operation write
    :path-pattern "**/*"
    :scope repo
    :default enabled
    :kill-switch none
    :audit ssot-checker)

  (effect
    :id ${projectId}-repo-file-append
    :feature ${projectId}-runtime-artifacts
    :kind filesystem-write
    :operation append
    :path-pattern "**/*"
    :scope repo
    :default enabled
    :kill-switch none
    :audit ssot-checker)

  (effect
    :id ${projectId}-repo-file-rename
    :feature ${projectId}-runtime-artifacts
    :kind filesystem-write
    :operation rename
    :path-pattern "**/*"
    :scope repo
    :default enabled
    :kill-switch none
    :audit ssot-checker)

  (effect
    :id ${projectId}-repo-file-delete
    :feature ${projectId}-runtime-artifacts
    :kind filesystem-write
    :operation delete
    :path-pattern "**/*"
    :scope repo
    :default enabled
    :kill-switch none
    :audit ssot-checker))
`;
}

function formatArray(values, indent) {
  if (values.length === 0) return '[]';
  const pad = ' '.repeat(indent);
  if (values.length === 1) return `[${quote(values[0])}]`;
  return `[${quote(values[0])}\n${values.slice(1).map((value) => `${pad}${quote(value)}`).join('\n')}]`;
}

function formatSymbolArray(values, indent) {
  if (values.length === 0) return '[]';
  const pad = ' '.repeat(indent);
  if (values.length === 1) return `[${values[0]}]`;
  return `[${values[0]}\n${values.slice(1).map((value) => `${pad}${value}`).join('\n')}]`;
}

function unique(values) {
  return [...new Set(values)].sort();
}

function quote(value) {
  return `"${String(value).replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

function fail(message) {
  console.error(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

main();
