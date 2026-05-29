#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';
import { maybeRunLispc, runLispc } from './lib/ocaml_lispc.mjs';

const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_REPO_ROOT = path.resolve(SCRIPT_DIR, '..');
const LEVEL_ORDER = new Map(
  Array.from({ length: 7 }, (_, index) => [`M${index}`, index]),
);

const usage = `Usage:
  node scripts/check-project-maturity.mjs [--json] [--min-level M5] [--project <id> ...]
  node scripts/check-project-maturity.mjs --evidence-only --min-level M6 --project <id>
  node scripts/check-project-maturity.mjs --dry-fixture [--json] [--engine=auto|js|ocaml]

Checks MissionD's project maturity registry against project-local Lisp SSOT
structure. The default gate proves every active/runtime-owned project is at
least M5 worker-operational SSOT closure. Incubating projects remain visible
with gaps, but do not block the global M5 gate until promoted. Use
--min-level M6 for Auth-grade final maturity: domain, policy, flow, event,
runtime, implementation, compatibility, hot-path wiring, and regression evidence.
Use --evidence-only inside worker shards before the central V3 maturity
registry is advanced.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repoRoot = opts.dryFixture ? buildFixture() : DEFAULT_REPO_ROOT;
  const result = runProjectMaturityCheck(repoRoot, opts);
  const engine = runOcamlMaturityCheck(repoRoot, opts);
  const projectDirEngine = runOcamlProjectDirChecks(repoRoot, result.projects ?? [], opts);
  result.engine = { ...engine, project_dirs: projectDirEngine };
  if (engine.strictResult) {
    result.ok = false;
    result.diagnostics.push(...(engine.strictResult.diagnostics ?? []).map((d) => ({
      file: d.file ?? BLUEPRINT_PATH,
      message: `${d.code ?? 'OCAML_PROJECT'}: ${d.message}`,
    })));
  } else if (engine.mode === 'ocaml' && engine.ok === false) {
    result.ok = false;
    result.diagnostics.push(...(engine.diagnostics ?? []).map((d) => ({
      file: d.file ?? BLUEPRINT_PATH,
      message: `${d.code ?? 'OCAML_PROJECT'}: ${d.message}`,
    })));
  }
  if (projectDirEngine.strictResult) {
    result.ok = false;
    result.diagnostics.push(...(projectDirEngine.strictResult.diagnostics ?? []).map((d) => ({
      file: d.file ?? 'project-dir',
      message: `${d.code ?? 'OCAML_PROJECT_DIR'}: ${d.message}`,
    })));
  } else if (projectDirEngine.mode === 'ocaml' && projectDirEngine.ok === false) {
    result.ok = false;
    result.diagnostics.push(...(projectDirEngine.diagnostics ?? []).map((d) => ({
      file: d.file ?? 'project-dir',
      message: `${d.code ?? 'OCAML_PROJECT_DIR'}: ${d.message}`,
    })));
  }

  if (opts.json) {
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } else if (result.ok) {
    console.log(
      `project maturity check OK (${result.projects.length} projects, min=${opts.minLevel})`,
    );
  } else {
    for (const d of result.diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`project maturity check FAILED -- ${result.diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    dryFixture: false,
    evidenceOnly: false,
    minLevel: 'M5',
    projectIds: [],
    engine: 'ocaml',
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--evidence-only') {
      opts.evidenceOnly = true;
    } else if (arg === '--min-level') {
      opts.minLevel = argv[++i] ?? fail('--min-level requires a value');
    } else if (arg.startsWith('--min-level=')) {
      opts.minLevel = arg.slice('--min-level='.length);
    } else if (arg === '--project') {
      opts.projectIds.push(argv[++i] ?? fail('--project requires a value'));
    } else if (arg.startsWith('--project=')) {
      opts.projectIds.push(arg.slice('--project='.length));
    } else if (arg === '--engine') {
      opts.engine = argv[++i] ?? fail('--engine requires a value');
    } else if (arg.startsWith('--engine=')) {
      opts.engine = arg.slice('--engine='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!LEVEL_ORDER.has(opts.minLevel)) fail(`unknown maturity level: ${opts.minLevel}`);
  if (!['auto', 'js', 'ocaml'].includes(opts.engine)) fail(`unknown engine: ${opts.engine}`);
  return opts;
}

function fail(message) {
  console.error(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

export function runProjectMaturityCheck(repoRoot, opts = {}) {
  const minLevel = opts.minLevel ?? 'M6';
  const diagnostics = [];
  const loaded = loadMaturityInputs(repoRoot, opts);
  diagnostics.push(...loaded.diagnostics);
  if (loaded.diagnostics.length > 0) {
    return {
      ok: false,
      source: loaded.source,
      min_level: minLevel,
      projects: [],
      diagnostics,
    };
  }
  const maturity = loaded.maturity;
  const projects = loaded.projects;

  const specialProjects = new Map([
    ['missiond', { id: 'missiond', root: repoRoot, intent: BLUEPRINT_PATH }],
  ]);
  for (const project of projects.values()) specialProjects.set(project.id, project);

  const selected = opts.projectIds?.length
    ? opts.projectIds
    : [...maturity.keys()];
  const rows = [];
  for (const id of selected) {
    const m = maturity.get(id);
    const registry = specialProjects.get(id);
    if (!m) {
      diagnostics.push({ file: BLUEPRINT_PATH, message: `missing maturity entry for ${id}` });
      continue;
    }
    const row = assessProject(repoRoot, id, m, registry);
    rows.push(row);
    diagnostics.push(...diagnosticsForProject(row, minLevel, {
      evidenceOnly: opts.evidenceOnly === true,
      explicitProject: (opts.projectIds?.length ?? 0) > 0,
    }));
  }

  return {
    ok: diagnostics.length === 0,
    source: loaded.source,
    min_level: minLevel,
    projects: rows,
    diagnostics,
  };
}

function loadMaturityInputs(repoRoot, opts) {
  if (opts.engine !== 'js' && opts.dryFixture !== true) {
    const result = runLispc([
      'emit-universe',
      '--blueprint',
      BLUEPRINT_PATH,
    ], { repoRoot });
    if (result?.ok === true && result.compiled?.payload) {
      return normalizeTypedMaturityInputs(result.compiled.payload, 'ocaml-emit-universe');
    }
    return {
      source: 'ocaml-emit-universe',
      maturity: new Map(),
      projects: new Map(),
      diagnostics: (result?.diagnostics ?? []).map((d) => ({
        file: d.file ?? BLUEPRINT_PATH,
        message: `${d.code ?? 'OCAML_EMIT_UNIVERSE'}: ${d.message}`,
      })),
    };
  }
  return loadMaturityInputsFromJs(repoRoot);
}

function normalizeTypedMaturityInputs(payload, source) {
  const diagnostics = [];
  const maturity = new Map();
  const projects = new Map();
  if (!Array.isArray(payload?.maturity)) {
    diagnostics.push({ file: source, message: 'typed universe projection missing maturity[]' });
  } else {
    for (const entry of payload.maturity) {
      if (!entry?.id) continue;
      maturity.set(entry.id, {
        id: entry.id,
        current: entry.current ?? 'M0',
        target: entry.target ?? 'M6',
        gap: Array.isArray(entry.gap) ? entry.gap : [],
      });
    }
  }
  if (!Array.isArray(payload?.projects)) {
    diagnostics.push({ file: source, message: 'typed universe projection missing projects[]' });
  } else {
    for (const project of payload.projects) {
      if (!project?.id) continue;
      projects.set(project.id, {
        id: project.id,
        kind: project.kind ?? null,
        management_domain: project.management_domain ?? null,
        runtime_layer: project.runtime_layer ?? null,
        root: project.root ?? null,
        path: project.path ?? null,
        intent: project.intent ?? null,
        backend: project.backend ?? null,
        frontend: project.frontend ?? null,
        operations: project.operations ?? null,
        checks: Array.isArray(project.checks) ? project.checks : [],
        status: project.status ?? null,
      });
    }
  }
  return { source, maturity, projects, diagnostics };
}

function loadMaturityInputsFromJs(repoRoot) {
  const diagnostics = [];
  const blueprintSource = readBlueprintWithEvidenceSidecars(repoRoot, BLUEPRINT_PATH);
  const forms = parseLisp(blueprintSource, BLUEPRINT_PATH);
  const root = forms.find((form) => isList(form) && head(form) === 'missiond-blueprint');
  if (!root) {
    return {
      source: 'js-blueprint-parser',
      maturity: new Map(),
      projects: new Map(),
      diagnostics: [{ file: BLUEPRINT_PATH, message: 'missing missiond-blueprint root' }],
    };
  }

  const maturityModel = firstChild(root, 'project-maturity-model');
  const maturityRegistry = firstChild(root, 'project-maturity-registry');
  const projectRegistry = firstChild(root, 'project-blueprint-registry');
  if (!maturityModel) diagnostics.push({ file: BLUEPRINT_PATH, message: 'missing project-maturity-model' });
  if (!maturityRegistry) diagnostics.push({ file: BLUEPRINT_PATH, message: 'missing project-maturity-registry' });
  if (!projectRegistry) diagnostics.push({ file: BLUEPRINT_PATH, message: 'missing project-blueprint-registry' });

  const maturity = maturityRegistry ? parseMaturityRegistry(maturityRegistry) : new Map();
  const projects = projectRegistry ? parseProjectRegistry(projectRegistry) : new Map();
  return { source: 'js-blueprint-parser', maturity, projects, diagnostics };
}

function runOcamlMaturityCheck(repoRoot, opts) {
  if (opts.dryFixture) {
    return { requested: opts.engine, mode: 'js-fixture', ok: true, diagnostics: [] };
  }
  if (opts.engine === 'js') return { requested: opts.engine, mode: 'js', ok: true, diagnostics: [] };
  const attempt = maybeRunLispc([
    'check-project',
    '--blueprint',
    BLUEPRINT_PATH,
    '--min-level',
    opts.minLevel ?? 'M6',
  ], { engine: opts.engine, repoRoot });
  if (attempt.mode === 'js-fallback') {
    return {
      requested: opts.engine,
      mode: 'js-fallback',
      ok: true,
      diagnostics: attempt.result?.diagnostics ?? [],
    };
  }
  const result = attempt.result;
  if (opts.engine === 'ocaml' && result?.unavailable) {
    return { requested: opts.engine, mode: 'ocaml', strictResult: result };
  }
  return {
    requested: opts.engine,
    mode: 'ocaml',
    ok: result?.ok === true,
    diagnostics: result?.diagnostics ?? [],
  };
}

function runOcamlProjectDirChecks(repoRoot, rows, opts) {
  if (opts.dryFixture) {
    return { requested: opts.engine, mode: 'js-fixture', ok: true, diagnostics: [], checked: [] };
  }
  if (opts.engine === 'js') return { requested: opts.engine, mode: 'js', ok: true, diagnostics: [], checked: [] };
  if (levelValue(opts.minLevel ?? 'M5') < 2) {
    return { requested: opts.engine, mode: 'ocaml-skipped', ok: true, diagnostics: [], checked: [] };
  }
  const command = levelValue(opts.minLevel ?? 'M5') >= 6 ? 'check-m6-depth' : 'check-project-dir';
  const diagnostics = [];
  const checked = [];
  const seen = new Set();
  for (const row of rows) {
    const dir = row.missiond_dir;
    if (!dir || seen.has(dir)) continue;
    seen.add(dir);
    const attempt = maybeRunLispc([command, '--dir', dir], {
      engine: opts.engine,
      repoRoot,
      timeoutMs: 60_000,
    });
    if (attempt.mode === 'js-fallback') {
      return {
        requested: opts.engine,
        mode: 'js-fallback',
        ok: true,
        diagnostics: attempt.result?.diagnostics ?? [],
        checked,
      };
    }
    const result = attempt.result;
    if (opts.engine === 'ocaml' && result?.unavailable) {
      return { requested: opts.engine, mode: 'ocaml', strictResult: result, checked };
    }
    checked.push({ project: row.id, dir, command, ok: result?.ok === true });
    if (!result?.ok) diagnostics.push(...(result?.diagnostics ?? []));
  }
  return {
    requested: opts.engine,
    mode: 'ocaml',
    ok: diagnostics.length === 0,
    diagnostics,
    checked,
  };
}

function firstChild(node, childHead) {
  if (!isList(node)) return null;
  return node.children.find((child) => isList(child) && head(child) === childHead) ?? null;
}

function parseMaturityRegistry(node) {
  const out = new Map();
  for (const child of node.children) {
    if (!isList(child) || head(child) !== 'maturity') continue;
    const props = readKeywordProps(child, { start: 1 });
    const id = nodeText(props[':id']?.value);
    if (!id) continue;
    out.set(id, {
      id,
      current: nodeText(props[':current']?.value) ?? 'M0',
      target: nodeText(props[':target']?.value) ?? 'M6',
      gap: vectorValues(props[':gap']?.value),
    });
  }
  return out;
}

function parseProjectRegistry(node) {
  const out = new Map();
  for (const child of node.children) {
    if (!isList(child) || head(child) !== 'project') continue;
    const props = readKeywordProps(child, { start: 1 });
    const id = nodeText(props[':id']?.value);
    if (!id) continue;
    out.set(id, {
      id,
      kind: nodeText(props[':kind']?.value) ?? null,
      management_domain: nodeText(props[':management-domain']?.value) ?? null,
      runtime_layer: nodeText(props[':runtime-layer']?.value) ?? null,
      root: nodeText(props[':root']?.value) ?? null,
      path: nodeText(props[':path']?.value) ?? null,
      intent: nodeText(props[':intent']?.value) ?? null,
      backend: nodeText(props[':backend']?.value) ?? null,
      frontend: nodeText(props[':frontend']?.value) ?? null,
      operations: nodeText(props[':operations']?.value) ?? null,
      checks: vectorValues(props[':checks']?.value),
      status: nodeText(props[':status']?.value) ?? null,
    });
  }
  return out;
}

function vectorValues(node) {
  if (!node || !isList(node)) return [];
  return node.children
    .map((child) => nodeText(child))
    .filter((value) => value != null && value !== '');
}

function assessProject(repoRoot, id, maturity, registry) {
  const root = registryRoot(repoRoot, registry);
  const files = lispFilesForProject(repoRoot, root, registry);
  const blueprintFiles = blueprintFilesForProject(repoRoot, root, registry);
  const text = files.map((file) => safeRead(file)).join('\n');
  const blueprintText = blueprintFiles.map((file) => safeRead(file)).join('\n');
  const structural = {
    lisp_files: files.length,
    blueprint_files: blueprintFiles.length,
    pillars: count(text, /\(pillar\b|:pillars\b/g),
    functions: count(text, /\(function\b|:functions\b/g),
    entry: count(text, /:entry\b|\(entry\b/g),
    core: count(text, /:core\b|\(core\b/g),
    steps: count(text, /\(step\b|\bstep\s+s\d+\b/g),
    egress: count(text, /:egress\b|\(egress\b/g),
    surfaces: count(text, /:surfaces?\b|\(surfaces?\b/g),
    runtime_projection: count(text, /:runtime-projection\b|\(runtime-projection\b|runtime-projection/g),
    event_bus: count(text, /event-bus|event bus|eventbus|event-stream|websocket|ExternalServiceEvent|missiond-event|commit-lisp-convergence|event-ingest/gi),
    worker_operational: count(text, /mission_swarm_run|context-pack|BoardTask|write_scope|write-scope|worker-operational|worker/gi),
    final_convergence: count(text, /final-convergence|final convergence gate|final-m6-report|M6|M10|v3-runtime-ssot/gi),
    m6_depth: count(text, /auth-grade|final-m6-report|domain-model|product-access-policy|policy-layer|flow-layer|event-contract|compatibility-ledger|hot-path-wiring|regression-matrix|runtime-registration|token-claim-contract/gi),
  };
  const evidence = {
    root_exists: root ? fs.existsSync(root) : id === 'missiond',
    intent_exists: intentExists(repoRoot, root, registry),
    has_project_blueprint: blueprintFiles.length > 0,
    has_checker: hasChecker(root, registry),
    has_lisp_shape: structural.entry > 0 && structural.core > 0 && structural.egress > 0 && structural.surfaces > 0,
    has_ordered_steps: structural.steps > 0,
    has_code_isomorphism: hasCodeIsomorphismEvidence(root, registry, text, blueprintText),
    has_runtime_projection: structural.runtime_projection > 0,
    has_event_loop: structural.event_bus > 0,
    has_worker_contract: structural.worker_operational > 0,
    has_final_convergence: structural.final_convergence > 0,
    has_m6_depth: hasM6DepthEvidence(text, blueprintText),
    has_behavior_closure: hasBehaviorClosureEvidence(repoRoot, id, root, text),
  };
  return {
    id,
    declared_current: maturity.current,
    target: maturity.target,
    declared_gap: maturity.gap,
    registry_status: registry?.status ?? (id === 'missiond' ? 'code-aligned' : null),
    management_domain: registry?.management_domain ?? (id === 'missiond' ? 'missiond-production-system' : null),
    runtime_layer: registry?.runtime_layer ?? (id === 'missiond' ? 'control-plane' : null),
    global_gate_exempt: isGlobalGateExempt(registry),
    root,
    missiond_dir: projectMissiondDir(repoRoot, root, registry, id),
    structural,
    evidence,
    evidence_level: evidenceLevel(evidence),
    next_gap: nextGap(maturity.current, evidence),
  };
}

function registryRoot(repoRoot, registry) {
  if (!registry) return null;
  if (registry.root) return registry.root;
  if (registry.path) return repoRoot;
  return null;
}

function projectMissiondDir(repoRoot, root, registry, id) {
  if (id === 'missiond') return null;
  if (registry?.path) {
    const file = path.resolve(root ?? repoRoot, registry.path);
    return fs.existsSync(file) && fs.statSync(file).isFile() ? path.dirname(file) : null;
  }
  if (!root) return null;
  const missiondDir = path.join(root, '.missiond');
  return fs.existsSync(missiondDir) ? missiondDir : null;
}

function lispFilesForProject(repoRoot, root, registry) {
  if (registry?.path) {
    const primary = path.resolve(repoRoot, registry.path);
    if (!fs.existsSync(primary)) return [];
    const stat = fs.statSync(primary);
    if (stat.isDirectory()) return listLispFiles(primary);
    return [...new Set([primary, ...listLispFiles(path.dirname(primary))])].sort();
  }
  if (!root || !fs.existsSync(root)) return [];
  const missiondDir = path.join(root, '.missiond');
  if (!fs.existsSync(missiondDir)) return [];
  return listLispFiles(missiondDir);
}

function blueprintFilesForProject(repoRoot, root, registry) {
  const out = new Set();
  for (const rel of [registry?.path, registry?.backend, registry?.frontend, registry?.operations]) {
    if (!rel) continue;
    const file = path.resolve(root ?? repoRoot, rel);
    if (fs.existsSync(file) && fs.statSync(file).isFile()) out.add(file);
  }
  if (root && fs.existsSync(path.join(root, '.missiond'))) {
    for (const file of listLispFiles(path.join(root, '.missiond'))) {
      if (path.basename(file).endsWith('blueprint.lisp')) out.add(file);
    }
  }
  if (registry?.path && root === repoRoot) {
    const file = path.resolve(repoRoot, registry.path);
    if (fs.existsSync(file) && path.basename(file).endsWith('blueprint.lisp')) out.add(file);
  }
  return [...out].sort();
}

function listLispFiles(dir) {
  const out = [];
  const stack = [dir];
  while (stack.length > 0) {
    const current = stack.pop();
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      if (entry.name === '.git' || entry.name === 'node_modules' || entry.name === 'target') continue;
      const file = path.join(current, entry.name);
      if (entry.isDirectory()) stack.push(file);
      else if (entry.isFile() && entry.name.endsWith('.lisp')) out.push(file);
    }
  }
  return out.sort();
}

function safeRead(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

function intentExists(repoRoot, root, registry) {
  if (registry?.path) return fs.existsSync(path.resolve(repoRoot, registry.path));
  if (!root) return false;
  const rel = registry?.intent ?? '.missiond/intent.lisp';
  return fs.existsSync(path.join(root, rel));
}

function hasChecker(root, registry) {
  if (registry?.checks?.length > 0) return true;
  if (!root) return false;
  return fs.existsSync(path.join(root, '.missiond/check.sh')) || fs.existsSync(path.join(root, 'scripts'));
}

function hasCodeIsomorphismEvidence(root, registry, allLispText, blueprintText) {
  const checks = (registry?.checks ?? []).join('\n');
  const source = `${checks}\n${blueprintText}\n${allLispText}`;
  if (/\b(code[-_ ]isomorphism|code[-_ ]aligned|current[-_ ]code[-_ ]mapping|current[-_ ]code[-_ ]mapped|code[-_ ]mapping|code[-_ ]mapped|surface[-_ ]map|implementation[-_ ]map)\b/i.test(source)) {
    return true;
  }
  if (root) {
    for (const rel of ['.missiond/check.sh', 'scripts/check-project-ssot.mjs']) {
      const file = path.join(root, rel);
      if (fs.existsSync(file) && /\b(code[-_ ]isomorphism|current[-_ ]code[-_ ]mapping|current[-_ ]code[-_ ]mapped|surface[-_ ]map)\b/i.test(safeRead(file))) {
        return true;
      }
    }
  }
  return false;
}

function hasM6DepthEvidence(allLispText, blueprintText) {
  const source = `${blueprintText}\n${allLispText}`.toLowerCase();
  const groups = [
    ['domain-model', 'domain model', 'authority-chain', 'authority chain'],
    ['policy', 'policy-layer', 'guard', 'capability'],
    ['flow', 'flow-layer', 'state-machine', 'state machine'],
    ['event-contract', 'event contract', 'event-bus', 'outbox'],
    ['runtime-projection', 'runtime projection', 'runtime-registration'],
    ['implementation-map', 'implementation map', 'code-isomorphism', 'current-code'],
    ['compatibility-ledger', 'compatibility ledger', 'legacy bridge'],
    ['hot-path-wiring', 'hot path', 'runtime caller'],
    ['regression-matrix', 'regression matrix', 'regression-tests', 'regression tests', 'backward compatibility'],
    ['final-m6-report', 'auth-grade', 'final-hardening-report', 'domain-hardening-report', 'final convergence', 'final-convergence'],
  ];
  return groups.every((needles) => needles.some((needle) => source.includes(needle)));
}

function hasBehaviorClosureEvidence(repoRoot, id, root, allLispText) {
  if (!allLispText.includes('(behavior-universe')) return false;
  const args = [
    path.join(SCRIPT_DIR, 'check-project-behavior-closure.mjs'),
    '--project',
    id,
    '--json',
  ];
  if (id !== 'missiond' && root) args.push('--root', root);
  const proc = spawnSync('node', args, {
    cwd: repoRoot,
    encoding: 'utf8',
    timeout: 60_000,
  });
  return proc.status === 0 && !proc.error;
}

function nextGap(current, evidence) {
  if (levelValue(current) < 1 || !evidence.intent_exists) return 'm1-registered-intent';
  if (levelValue(current) < 2 || !evidence.has_project_blueprint || !evidence.has_lisp_shape || !evidence.has_ordered_steps) return 'm2-blueprint-split';
  if (levelValue(current) < 3 || !evidence.has_code_isomorphism) return 'm3-code-mapping';
  if (levelValue(current) < 4 || !evidence.has_runtime_projection || !evidence.has_event_loop) return 'm4-runtime-projection';
  if (levelValue(current) < 5 || !evidence.has_worker_contract || !evidence.has_final_convergence) return 'm5-worker-operational';
  if (levelValue(current) < 6 || !evidence.has_m6_depth || !evidence.has_behavior_closure) return 'm6-auth-grade-depth';
  return null;
}

function diagnosticsForProject(row, minLevel, { evidenceOnly = false, explicitProject = false } = {}) {
  const diagnostics = [];
  const file = row.root ?? row.id;
  if (!row.evidence.root_exists) diagnostics.push({ file, message: `${row.id} root does not exist` });
  if (!row.evidence.intent_exists) diagnostics.push({ file, message: `${row.id} intent/blueprint entry does not exist` });
  if (row.structural.lisp_files === 0) diagnostics.push({ file, message: `${row.id} has no Lisp SSOT files` });
  if (levelValue(minLevel) >= 2 && !row.evidence.has_checker) {
    diagnostics.push({ file, message: `${row.id} has no declared/local checker` });
  }
  if (levelValue(minLevel) >= 2 && !row.evidence.has_project_blueprint) {
    diagnostics.push({ file, message: `${row.id} has no project-level *blueprint.lisp; intent-only projects cannot satisfy M2+` });
  }

  if (
    row.global_gate_exempt === true
    && explicitProject !== true
    && evidenceOnly !== true
    && levelValue(minLevel) >= 5
  ) {
    if (row.declared_gap.length === 0) {
      diagnostics.push({ file, message: `${row.id} is incubating below M5 but declares no gap` });
    }
    return diagnostics;
  }

  if (evidenceOnly) {
    if (levelValue(row.evidence_level) < levelValue(minLevel)) {
      diagnostics.push({
        file,
        message: `${row.id} evidence level ${row.evidence_level}, below required ${minLevel}; declared current remains ${row.declared_current}`,
      });
    }
    return diagnostics;
  }

  if (levelValue(row.declared_current) < levelValue(minLevel)) {
    diagnostics.push({
      file,
      message: `${row.id} declared ${row.declared_current}, below required ${minLevel}; next gap ${row.next_gap}`,
    });
    return diagnostics;
  }

  if (levelValue(row.evidence_level) < levelValue(minLevel)) {
    diagnostics.push({
      file,
      message: `${row.id} evidence level ${row.evidence_level}, below required ${minLevel}; declared current is ${row.declared_current}; next gap ${row.next_gap}`,
    });
  }
  if (levelValue(minLevel) >= 2 && !row.evidence.has_lisp_shape) {
    diagnostics.push({ file, message: `${row.id} lacks entry/core/egress/surface SSOT shape required for M2+` });
  }
  if (levelValue(minLevel) >= 2 && !row.evidence.has_ordered_steps) {
    diagnostics.push({ file, message: `${row.id} lacks ordered core steps required for M2+` });
  }
  if (levelValue(minLevel) >= 3 && !row.evidence.has_code_isomorphism) {
    diagnostics.push({ file, message: `${row.id} lacks code-isomorphism/current-code-mapping evidence required for M3+` });
  }
  if (levelValue(minLevel) >= 4 && !row.evidence.has_runtime_projection) {
    diagnostics.push({ file, message: `${row.id} missing M4 runtime projection evidence` });
  }
  if (levelValue(minLevel) >= 4 && !row.evidence.has_event_loop) {
    diagnostics.push({ file, message: `${row.id} missing M4 event-contract evidence` });
  }
  if (levelValue(minLevel) >= 5 && !row.evidence.has_worker_contract) {
    diagnostics.push({ file, message: `${row.id} missing M5 worker-operational evidence` });
  }
  if (levelValue(minLevel) >= 5 && !row.evidence.has_final_convergence) {
    diagnostics.push({ file, message: `${row.id} missing M5 final-convergence evidence` });
  }
  if (levelValue(minLevel) >= 6 && !row.evidence.has_m6_depth) {
    diagnostics.push({ file, message: `${row.id} missing M6 Auth-grade depth evidence` });
  }
  if (levelValue(minLevel) >= 6 && !row.evidence.has_behavior_closure) {
    diagnostics.push({ file, message: `${row.id} missing M6 program-level behavior closure` });
  }
  if (levelValue(row.declared_current) < 6 && row.declared_gap.length === 0) {
    diagnostics.push({ file, message: `${row.id} is below M6 but declares no gap` });
  }
  return diagnostics;
}

function isGlobalGateExempt(registry) {
  const status = (registry?.status ?? '').toString().trim().toLowerCase();
  return status === 'incubating-project' || status === 'incubating';
}

function evidenceLevel(evidence) {
  if (!evidence.root_exists || !evidence.intent_exists) return 'M0';
  if (!evidence.has_checker || !evidence.has_project_blueprint || !evidence.has_lisp_shape || !evidence.has_ordered_steps) return 'M1';
  if (!evidence.has_code_isomorphism) return 'M2';
  if (!evidence.has_runtime_projection || !evidence.has_event_loop) return 'M3';
  if (!evidence.has_worker_contract || !evidence.has_final_convergence) return 'M4';
  if (!evidence.has_m6_depth || !evidence.has_behavior_closure) return 'M5';
  return 'M6';
}

function levelValue(level) {
  return LEVEL_ORDER.get(level) ?? -1;
}

function count(text, re) {
  return (text.match(re) ?? []).length;
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-project-maturity-'));
  fs.mkdirSync(path.join(root, '.missiond/v3'), { recursive: true });
  fs.mkdirSync(path.join(root, 'scripts'), { recursive: true });
  fs.mkdirSync(path.join(root, 'projects/app/.missiond/backend'), { recursive: true });
  fs.writeFileSync(path.join(root, '.missiond/v3/missiond-blueprint.lisp'), `(missiond-blueprint
  :code-isomorphism current-code-mapping
  (pillar runtime
    (function missiond-runtime
      :entry [mission_request]
      :core ((step s1 :logic "read blueprint") (step s2 :logic "run checker"))
      :egress [runtime-status]
      :surfaces ["src/main.rs"]
      :runtime-projection [runtime-policy]))
  (project-maturity-model
    :schema "missiond.project-maturity-model.v2"
    :levels ((level M5 :name worker-operational) (level M6 :name auth-grade)))
  (project-maturity-registry
    :schema "missiond.project-maturity-registry.v2"
    (maturity :id missiond :current M5 :target M6 :gap [final-m6-report])
    (maturity :id app :current M5 :target M6 :gap [final-m6-report]))
  (project-blueprint-registry
    (project :id app :root "${path.join(root, 'projects/app')}" :intent ".missiond/intent.lisp" :backend ".missiond/backend/app-backend-blueprint.lisp" :checks ["bash .missiond/check.sh"])))`);
  fs.writeFileSync(path.join(root, '.missiond/behavior-universe.lisp'), `(behavior-universe missiond
  :schema "missiond.behavior-universe.v1"
  :project missiond
  (behavior :id missiond-fixture :kind worker :owner test :observed ["worker:*"] :code ["src/main.rs"] :effects []))`);
  fs.writeFileSync(path.join(root, 'projects/app/.missiond/intent.lisp'), `(project app
  :pillars [api]
  :maturity M6
  (function app-api
    :pillar api
    :entry [HTTP]
    :core ((step s1 :logic "read") (step s2 :logic "write"))
    :egress [JSON]
    :surfaces ["src/main.rs"]
    :runtime-projection [APP_PORT]))`);
  fs.writeFileSync(path.join(root, 'projects/app/.missiond/backend/app-backend-blueprint.lisp'), `(project-backend-blueprint app
  :schema "project.backend-blueprint.v1"
  :code-isomorphism current-code-mapped
  (pillar api
    (function app-api
      :entry [HTTP]
      :core ((step s1 :logic "read") (step s2 :logic "write"))
      :egress [JSON]
      :surfaces ["src/main.rs"]
      :runtime-projection [APP_PORT])))`);
  fs.writeFileSync(path.join(root, 'projects/app/.missiond/behavior-universe.lisp'), `(behavior-universe app
  :schema "missiond.behavior-universe.v1"
  :project app
  (behavior :id app-fixture :kind route :owner test :observed ["route:*"] :code ["src/main.rs"] :effects []))`);
  fs.writeFileSync(path.join(root, 'projects/app/.missiond/check.sh'), '#!/usr/bin/env bash\nexit 0\n');
  return root;
}

main();
