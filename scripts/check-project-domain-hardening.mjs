#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { runLispc } from './lib/ocaml_lispc.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..');
const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';
const LEVEL_ORDER = new Map(['H0', 'H1', 'H2', 'H3', 'H4', 'H5'].map((level, index) => [level, index]));

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const result = runCheck(opts);
  if (opts.json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log(`project domain hardening check OK (${result.projects.length} projects, min=${opts.minLevel})`);
  else {
    for (const d of result.diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`project domain hardening check FAILED -- ${result.diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    minLevel: 'H0',
    projectIds: [],
    summary: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--summary') opts.summary = true;
    else if (arg === '--min-level') opts.minLevel = argv[++i] ?? fail('--min-level requires a value');
    else if (arg.startsWith('--min-level=')) opts.minLevel = arg.slice('--min-level='.length);
    else if (arg === '--project') opts.projectIds.push(argv[++i] ?? fail('--project requires a value'));
    else if (arg.startsWith('--project=')) opts.projectIds.push(arg.slice('--project='.length));
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/check-project-domain-hardening.mjs [--json] [--summary] [--min-level H0..H5] [--project <id>]');
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!LEVEL_ORDER.has(opts.minLevel)) fail(`unknown hardening level: ${opts.minLevel}`);
  return opts;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

function runCheck(opts) {
  const diagnostics = [];
  const universe = loadUniverse();
  diagnostics.push(...universe.diagnostics);
  const registry = parseHardeningRegistry();
  diagnostics.push(...registry.diagnostics);

  const selectedIds = opts.projectIds.length
    ? opts.projectIds
    : [...new Set([...registry.rows.keys(), ...universe.projects.map((p) => p.id)])];
  const rows = [];
  for (const id of selectedIds) {
    const declared = registry.rows.get(id);
    const project = universe.projects.find((p) => p.id === id);
    if (!declared) {
      diagnostics.push({ file: BLUEPRINT_PATH, message: `missing domain-hardening entry for ${id}` });
      continue;
    }
    if (!project && id !== 'missiond') {
      diagnostics.push({ file: BLUEPRINT_PATH, message: `domain-hardening project ${id} is not registered in project-blueprint-registry` });
      continue;
    }
    const missiondDir = resolveMissiondDir(project, id);
    const structural = missiondDir ? runDomainStructuralGate(missiondDir) : missingDirResult(id);
    const structuralClaimed = levelValue(declared.current) >= levelValue('H2');
    const structuralRequired = levelValue(opts.minLevel) >= levelValue('H2');
    const evidenceLevel = structural.ok || !structuralClaimed ? declared.current : lowerLevel(declared.current, 'H1');
    const row = opts.summary
      ? {
          id,
          current: declared.current,
          target: declared.target,
          evidence_level: evidenceLevel,
          gap_count: declared.gap.length,
          structural: summarizeStructural(structural),
        }
      : {
          id,
          current: declared.current,
          target: declared.target,
          gap: declared.gap,
          missiond_dir: missiondDir,
          structural,
          evidence_level: evidenceLevel,
        };
    rows.push(row);
    if (levelValue(declared.current) < levelValue(opts.minLevel)) {
      diagnostics.push({ file: BLUEPRINT_PATH, message: `${id} domain hardening is ${declared.current}, below required ${opts.minLevel}` });
    }
    if (!structural.ok && (structuralClaimed || structuralRequired)) {
      for (const d of structural.diagnostics) diagnostics.push({ file: d.file ?? missiondDir ?? id, message: `${d.code ?? 'DOMAIN'}: ${d.message}` });
    }
    if (levelValue(evidenceLevel) < levelValue(opts.minLevel)) {
      diagnostics.push({ file: missiondDir ?? BLUEPRINT_PATH, message: `${id} domain hardening evidence is ${evidenceLevel}, below required ${opts.minLevel}` });
    }
  }
  return {
    ok: diagnostics.length === 0,
    min_level: opts.minLevel,
    projects: rows,
    diagnostics,
  };
}

function summarizeStructural(structural) {
  return {
    ok: structural.ok === true,
    engine: structural.engine ?? 'ocaml',
    diagnostic_count: structural.diagnostics?.length ?? 0,
  };
}

function loadUniverse() {
  const result = runLispc(['emit-universe', '--blueprint', BLUEPRINT_PATH], { repoRoot: REPO_ROOT, timeoutMs: 60_000 });
  if (!result.ok || !result.compiled?.payload?.projects) {
    return {
      projects: [],
      diagnostics: (result.diagnostics ?? []).map((d) => ({ file: d.file ?? BLUEPRINT_PATH, message: `${d.code ?? 'UNIVERSE'}: ${d.message}` })),
    };
  }
  return { projects: result.compiled.payload.projects, diagnostics: [] };
}

function parseHardeningRegistry() {
  const text = fs.readFileSync(path.join(REPO_ROOT, BLUEPRINT_PATH), 'utf8');
  const start = text.indexOf('(project-domain-hardening-registry');
  const end = text.indexOf('(project-blueprint-registry', start);
  const match = start >= 0 && end > start ? [text.slice(start, end)] : null;
  if (!match) {
    return {
      rows: new Map(),
      diagnostics: [{ file: BLUEPRINT_PATH, message: 'missing project-domain-hardening-registry' }],
    };
  }
  const body = match[0];
  const rows = new Map();
  const re = /\(hardening\s+:id\s+([^\s)]+)\s+:current\s+(H[0-5])\s+:target\s+(H[0-5])\s+:gap\s+\[([^\]]*)\]/g;
  let m;
  while ((m = re.exec(body)) !== null) {
    const gap = m[4].trim() ? m[4].trim().split(/\s+/) : [];
    rows.set(m[1], { id: m[1], current: m[2], target: m[3], gap });
  }
  return { rows, diagnostics: rows.size ? [] : [{ file: BLUEPRINT_PATH, message: 'project-domain-hardening-registry has no hardening rows' }] };
}

function resolveMissiondDir(project, id) {
  if (id === 'missiond') return path.join(REPO_ROOT, '.missiond');
  if (!project) return null;
  const root = project.root ? absoluteFromRepo(project.root) : REPO_ROOT;
  const candidate = project.path ?? project.intent ?? project.backend ?? project.frontend ?? project.operations;
  if (!candidate) return null;
  const absolute = absoluteFromRoot(root, candidate);
  return fs.existsSync(absolute) && fs.statSync(absolute).isDirectory()
    ? absolute
    : path.dirname(absolute);
}

function runDomainStructuralGate(dir) {
  if (!fs.existsSync(dir)) {
    return { ok: false, diagnostics: [{ file: dir, code: 'DOMAIN_DIR_MISSING', message: 'project .missiond directory does not exist' }] };
  }
  const result = runLispc(['check-domain-hardening', '--dir', dir], { repoRoot: REPO_ROOT, timeoutMs: 60_000 });
  return {
    ok: result.ok === true,
    diagnostics: result.diagnostics ?? [],
    engine: result.unavailable ? 'unavailable' : 'ocaml',
  };
}

function missingDirResult(id) {
  return {
    ok: false,
    diagnostics: [{ file: id, code: 'DOMAIN_DIR_UNKNOWN', message: 'cannot resolve project .missiond directory' }],
  };
}

function absoluteFromRepo(value) {
  return path.isAbsolute(value) ? value : path.resolve(REPO_ROOT, value);
}

function absoluteFromRoot(root, value) {
  return path.isAbsolute(value) ? value : path.resolve(root, value);
}

function levelValue(level) {
  return LEVEL_ORDER.get(level) ?? 0;
}

function lowerLevel(left, right) {
  return levelValue(left) < levelValue(right) ? left : right;
}

main();
