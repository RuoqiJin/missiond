#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';

import {
  BEHAVIOR_UNIVERSE_SCANNER_VERSION,
  COMPILED_BEHAVIOR_NAVIGATION_SCHEMA_VERSION,
  behaviorNavigationRuntimeTarget,
  behaviorNavigationSourceHash,
  behaviorNavigationSourceUnits,
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
  node scripts/propose-behavior-navigation.mjs --project <id> [--json] [--write] [--root <path>] [--repo <path>] [--legacy-write-lisp]

Generates deterministic navigation anchors from scanner output. By default,
--write stores a compiled runtime artifact. --legacy-write-lisp keeps the old
compatibility path that writes generated anchors into project behavior-universe
Lisp.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const result = generateBehaviorNavigation({
    project: opts.project,
    root: opts.root,
    repo: opts.repo,
  });

  if (opts.write) {
    if (opts.legacyWriteLisp && !result.missiondV3) {
      writeNavigationForms(result.legacyTarget, opts.project, result.root, result.forms);
      result.legacy_written = true;
    } else {
      writeCompiledBehaviorNavigation(result.target, result.artifact);
      result.legacy_written = false;
    }
    if (result.missiondV3) {
      stripMissiondV3NavigationBlock(result.root);
    }
    result.written = true;
  } else {
    result.written = false;
    result.legacy_written = false;
  }

  if (opts.json) {
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } else {
    console.log(`${opts.project}: proposed ${result.anchor_count} navigation anchor(s) for ${result.target}`);
    if (opts.write) console.log(`${opts.project}: wrote navigation anchors`);
  }
}

export function generateBehaviorNavigation({
  project,
  root = null,
  repo = process.cwd(),
  target = null,
}) {
  const projectRoot = root ?? resolveProjectRoot(repo, project);
  if (!projectRoot) fail(`cannot resolve project root for ${project}`);
  if (!fs.existsSync(projectRoot)) fail(`project root does not exist: ${projectRoot}`);

  const missiondV3 = project === 'missiond';
  const observed = scanObservedUniverse(projectRoot, { projectId: project });
  const declared = loadDeclaredBehaviorUniverse(projectRoot, { projectId: project, missiondV3 });
  const riskItems = observed.filter(isNavigationRisk);
  const anchors = navigationAnchorsForItems(riskItems);
  const navigationForms = generateNavigationForms(project, anchors);
  const outputTarget = target ?? behaviorUniverseTarget(projectRoot, { projectId: project, missiondV3 });
  const artifact = compiledBehaviorNavigationArtifact({
    projectId: project,
    root: projectRoot,
    target: outputTarget,
    observed,
    riskItems,
    anchors,
    navigationForms,
  });
  return {
    ok: true,
    projectId: project,
    root: projectRoot,
    target: outputTarget,
    missiondV3,
    observed_count: observed.length,
    risk_count: riskItems.length,
    anchor_count: anchors.length,
    effect_anchor_count: riskItems.filter((item) => item.kind === 'effect').length,
    declared_effect_count: declared.effects.length,
    forms: navigationForms.trimEnd(),
    artifact,
    legacyTarget: path.join(projectRoot, '.missiond/behavior-universe.lisp'),
  };
}

function parseArgs(argv) {
  const opts = {
    json: false,
    write: false,
    repo: process.cwd(),
    root: null,
    project: null,
    legacyWriteLisp: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--write') opts.write = true;
    else if (arg === '--legacy-write-lisp') opts.legacyWriteLisp = true;
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

function behaviorUniverseTarget(root, { projectId, missiondV3 }) {
  if (missiondV3 || projectId !== 'missiond') {
    return behaviorNavigationRuntimeTarget(projectId);
  }
  return path.join(root, '.missiond/behavior-universe.lisp');
}

function isNavigationRisk(item) {
  return NAVIGATION_RISK_KINDS.has(item.kind)
    || (item.kind === 'effect' && item.scope === 'external-home')
    || (item.kind === 'effect' && Boolean(item.effectHint));
}

function navigationAnchorsForItems(items) {
  const byKey = new Map();
  for (const item of items) {
    const role = item.role ?? roleForKind(item.kind);
    const semanticId = navigationObservedId(item);
    const key = item.stability === 'line-bound'
      ? `${behaviorKindFor(item)}\0${role}\0${item.file}\0${item.id}\0${item.effectHint ?? ''}`
      : `${behaviorKindFor(item)}\0${role}\0${item.file}\0${item.symbol ?? ''}\0${item.effectHint ?? ''}`;
    const current = byKey.get(key);
    const legacyObservedIds = unique([
      ...(current?.legacy_observed_ids ?? []),
      item.legacy_id ?? item.id,
    ]);
    byKey.set(key, {
      id: semanticId,
      semantic_id: item.semantic_id ?? semanticId,
      legacy_observed_id: legacyObservedIds[0] ?? item.id,
      legacy_observed_ids: legacyObservedIds,
      kind: behaviorKindFor(item),
      observed_kind: item.kind,
      role,
      file: item.file,
      line: Math.min(current?.line ?? item.line ?? 1, item.line ?? 1),
      symbol: item.symbol ?? null,
      effect: item.effectHint ?? null,
      stability: item.stability ?? 'line-bound',
    });
  }
  return [...byKey.values()].sort((a, b) => (
    `${a.kind}\0${a.file}\0${a.symbol ?? ''}\0${a.id}`.localeCompare(`${b.kind}\0${b.file}\0${b.symbol ?? ''}\0${b.id}`)
  ));
}

function generateNavigationForms(projectId, anchors) {
  const groups = new Map();
  for (const item of anchors) {
    const kind = item.kind;
    if (!groups.has(kind)) groups.set(kind, []);
    groups.get(kind).push(item);
  }

  const forms = [];
  for (const [kind, group] of [...groups.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    const id = `${projectId}-navigation-${kind}`;
    const observed = unique(group.map((item) => item.id));
    const code = unique(group.map((item) => item.file));
    const effects = unique(group.map((item) => item.effect).filter(Boolean));
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
  const parts = [
    `      :role ${item.role}`,
    `      :observed ${quote(item.id)}`,
    `      :semantic-id ${quote(item.semantic_id)}`,
    `      :legacy-observed-id ${quote(item.legacy_observed_id)}`,
    `      :file ${quote(item.file)}`,
  ];
  if (item.symbol) parts.push(`      :symbol ${quote(item.symbol)}`);
  if (item.effect) parts.push(`      :effect ${item.effect}`);
  return `    (anchor
${parts.join('\n')})`;
}

function navigationObservedId(item) {
  if (item.semantic_id) return item.semantic_id;
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
  const navigationSection = `${navigationForms.trimEnd()}\n`;
  const effectIndex = nextText.indexOf('\n  (effect\n');
  if (effectIndex >= 0) {
    return `${nextText.slice(0, effectIndex)}\n${navigationSection}${nextText.slice(effectIndex)}`;
  }
  const closeIndex = nextText.lastIndexOf(')');
  if (closeIndex < 0) return `${nextText.trimEnd()}\n${navigationSection}`;
  return `${nextText.slice(0, closeIndex).trimEnd()}\n\n${navigationSection}${nextText.slice(closeIndex)}`;
}

function compiledBehaviorNavigationArtifact({
  projectId,
  root,
  target,
  observed,
  riskItems,
  anchors,
  navigationForms,
}) {
  const sourceUnits = behaviorNavigationSourceUnits(riskItems);
  return {
    schema_version: COMPILED_BEHAVIOR_NAVIGATION_SCHEMA_VERSION,
    project_id: projectId,
    root,
    target,
    scanner_version: BEHAVIOR_UNIVERSE_SCANNER_VERSION,
    source_hash: behaviorNavigationSourceHash({ projectId, root, sourceUnits }),
    generated_at: null,
    observed_count: observed.length,
    risk_count: riskItems.length,
    anchor_count: anchors.length,
    source_units: sourceUnits,
    anchors,
    legacy: {
      forms: navigationForms.trimEnd(),
    },
    diagnostics: [],
  };
}

function defaultBehaviorUniverse(projectId, navigationForms = '') {
  const navigationSection = navigationForms.trim()
    ? `${navigationForms.trimEnd()}\n\n`
    : '';
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

${navigationSection}  (effect
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

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
