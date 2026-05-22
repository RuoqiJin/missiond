#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';

const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const COMPILED_SEMANTIC_IR = '.missiond/v3/runtime/compiled/compiled-semantic-ir.json';
const COMPILED_WORKFLOWS = '.missiond/v3/runtime/compiled/compiled-workflows.json';
const FINAL_CONVERGENCE_COMMAND = [
  'node',
  'scripts/check-v3-final-convergence.mjs',
  '--json',
  '--static-only',
];

const TOP_LEVEL_LINE_LIMIT = 250;
const NOTE_CHAR_LIMIT = 900;
const FACADE_NEAR_LIMIT_RATIO = 0.8;
const SURFACE_FLOW_GAP_ALLOWLIST = new Set([
  'missiond-blue-green-self-update',
]);

const usage = `Usage:
  node scripts/analyze-v3-self-evolution.mjs [--json] [--repo <path>]
  node scripts/analyze-v3-self-evolution.mjs --dry-fixture [--json]
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    const result = runFixtures();
    if (opts.json) console.log(JSON.stringify(result, null, 2));
    else console.log(`analyze-v3-self-evolution fixtures OK (${result.cases} cases)`);
    process.exit(result.ok ? 0 : 1);
  }

  const result = analyzeRepo(opts.repo);
  if (opts.json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) {
    console.log(`self-evolution analyzer OK (${result.findings.length} finding(s))`);
  } else {
    for (const diagnostic of result.diagnostics) console.error(diagnostic);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    dryFixture: false,
    repo: process.cwd(),
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--dry-fixture') opts.dryFixture = true;
    else if (arg === '--repo') opts.repo = argv[++i] ?? fail('--repo requires a value');
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function fail(message) {
  console.error(`${message}\n\n${usage}`);
  process.exit(2);
}

function analyzeRepo(repo) {
  const diagnostics = [];
  const blueprintPath = path.join(repo, BLUEPRINT);
  const semanticPath = path.join(repo, COMPILED_SEMANTIC_IR);
  const workflowsPath = path.join(repo, COMPILED_WORKFLOWS);

  const blueprintSource = readText(blueprintPath, diagnostics, BLUEPRINT);
  const semantic = readJson(semanticPath, diagnostics, COMPILED_SEMANTIC_IR);
  const workflows = readJson(workflowsPath, diagnostics, COMPILED_WORKFLOWS);
  const finalConvergence = runFinalConvergence(repo, diagnostics);
  const authoringSources = readAuthoringSources(repo, semantic, blueprintSource, diagnostics);

  if (!blueprintSource || !semantic || !workflows || !finalConvergence) {
    return { ok: false, findings: [], diagnostics };
  }

  const findings = buildFindings({
    blueprintSource,
    semantic,
    workflows,
    finalConvergence,
    authoringSources,
    file: BLUEPRINT,
  });
  return { ok: diagnostics.length === 0, findings, diagnostics };
}

function readText(file, diagnostics, label) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch (err) {
    diagnostics.push(`${label}: cannot read: ${err.message}`);
    return null;
  }
}

function readJson(file, diagnostics, label) {
  const raw = readText(file, diagnostics, label);
  if (raw == null) return null;
  try {
    return JSON.parse(raw);
  } catch (err) {
    diagnostics.push(`${label}: cannot parse JSON: ${err.message}`);
    return null;
  }
}

function runFinalConvergence(repo, diagnostics) {
  const result = spawnSync(FINAL_CONVERGENCE_COMMAND[0], FINAL_CONVERGENCE_COMMAND.slice(1), {
    cwd: repo,
    encoding: 'utf8',
    timeout: 60_000,
  });
  if (result.error) {
    diagnostics.push(`final convergence command failed: ${result.error.message}`);
    return null;
  }
  const stdout = result.stdout ?? '';
  try {
    return JSON.parse(stdout);
  } catch (err) {
    diagnostics.push(`final convergence command returned invalid JSON: ${err.message}`);
    return {
      ok: false,
      failed_stage: 'parse-final-convergence-json',
      stdout_tail: tail(stdout, 2000),
      stderr_tail: tail(result.stderr ?? '', 1000),
      exit_code: result.status,
    };
  }
}

export function buildFindings({
  blueprintSource,
  semantic,
  workflows,
  finalConvergence,
  authoringSources = null,
  file = BLUEPRINT,
}) {
  const findings = [];
  findings.push(...detectFinalConvergenceBlocker(finalConvergence));
  findings.push(...detectFacadeBudgetNearLimit(finalConvergence));
  findings.push(
    ...detectOversizedAuthoringBlocks(
      authoringSources ?? [{ file, source: blueprintSource }],
    ),
  );
  findings.push(...detectSurfaceFlowGaps(semantic));
  // Touch workflow payload so the input remains intentional and checker-pinned.
  if (!Array.isArray(workflows?.payload?.workflows)) {
    findings.push(finding({
      id: 'compiled-workflow-contracts-missing',
      className: 'safe-backfill',
      risk: 'low',
      summary: 'Compiled workflow runtime projection is missing structured workflows.',
      evidenceRefs: [COMPILED_WORKFLOWS],
      affectedSurfaces: ['nightly-evolution-loop'],
      recommendedChange: 'Re-run typed Lisp runtime compile and repair emit-workflows projection if workflow payload remains empty.',
      acceptance: ['node scripts/compile-v3-runtime.mjs --json', 'node scripts/check-typed-lisp-compiler.mjs'],
      nonGoals: ['Do not read Board history or provider logs to repair workflow projection.'],
    }));
  }
  return stableFindings(findings);
}

function detectFinalConvergenceBlocker(finalConvergence) {
  if (finalConvergence?.ok === true) return [];
  const failedStage = finalConvergence?.failed_stage ?? 'unknown';
  return [finding({
    id: 'final-convergence-blocker',
    className: 'safe-backfill',
    risk: 'low',
    summary: `Final convergence static snapshot is not green: ${failedStage}.`,
    evidenceRefs: ['node scripts/check-v3-final-convergence.mjs --json --static-only'],
    affectedSurfaces: ['final-convergence'],
    recommendedChange: 'Create a narrow Lisp/checker backfill proposal for the blocking final convergence stage before any code mutation.',
    acceptance: ['node scripts/check-v3-final-convergence.mjs --json --static-only'],
    nonGoals: ['Do not dispatch implementation workers from this analyzer.', 'Do not inspect KB, Board history, provider logs, or worker telemetry.'],
  })];
}

function detectFacadeBudgetNearLimit(finalConvergence) {
  const near = (finalConvergence?.facades ?? [])
    .filter((facade) => {
      const lines = Number(facade.lines ?? 0);
      const maxLines = Number(facade.maxLines ?? 0);
      return maxLines > 0 && lines / maxLines >= FACADE_NEAR_LIMIT_RATIO;
    })
    .sort((a, b) => String(a.id).localeCompare(String(b.id)));
  if (near.length === 0) return [];
  return [finding({
    id: 'facade-budget-near-limit',
    className: 'architecture-proposal',
    risk: 'low',
    summary: `Facade line budgets are above ${Math.round(FACADE_NEAR_LIMIT_RATIO * 100)}% for ${near.length} surface(s).`,
    evidenceRefs: near.map((facade) => `${facade.file}: ${facade.lines}/${facade.maxLines} lines`),
    affectedSurfaces: near.map((facade) => String(facade.id ?? facade.file)),
    recommendedChange: 'Prepare a split proposal for near-limit facades, keeping public surface behavior unchanged and adding checker pins before implementation.',
    acceptance: ['node scripts/check-v3-final-convergence.mjs --json --static-only'],
    nonGoals: ['Do not run formatter or refactor code from the analyzer.', 'Do not create exact implementation shards automatically.'],
  })];
}

function readAuthoringSources(repo, semantic, fallbackSource, diagnostics) {
  const units = semantic?.payload?.source_units;
  if (!Array.isArray(units) || units.length === 0) {
    return [{ file: BLUEPRINT, source: fallbackSource }];
  }
  return units
    .filter((unit) => typeof unit?.file === 'string' && unit.file.trim() !== '')
    .map((unit) => {
      const rel = unit.file;
      const source = readText(path.join(repo, rel), diagnostics, rel);
      return source ? { file: rel, source } : null;
    })
    .filter(Boolean);
}

function detectOversizedAuthoringBlocks(authoringSources) {
  const findings = authoringSources.flatMap(({ source, file }) =>
    detectOversizedAuthoringBlocksInSource(source, file),
  );
  if (findings.length <= 1) return findings;
  return [finding({
    id: 'oversized-authoring-block',
    className: 'architecture-proposal',
    risk: findings.some((row) => row.risk === 'medium' || row.risk === 'high')
      ? 'medium'
      : 'low',
    summary: `V3 authoring density exceeds limits across ${findings.length} source unit(s).`,
    evidenceRefs: unique(findings.flatMap((row) => row.evidenceRefs)).slice(0, 12),
    affectedSurfaces: unique(findings.flatMap((row) => row.affectedSurfaces)),
    recommendedChange: 'Move long explanatory evidence into sidecars and keep active V3 authoring blocks focused on executable entry/core/egress contracts.',
    acceptance: ['node scripts/check-v3-final-convergence.mjs --json --static-only', 'node scripts/check-lisp-blueprint-compression.mjs'],
    nonGoals: ['Do not delete evidence.', 'Do not compress runtime report directories or cold evidence by default.'],
  })];
}

function detectOversizedAuthoringBlocksInSource(source, file) {
  let root;
  try {
    const forms = parseLisp(source, file);
    root = forms.find((form) => isList(form) && head(form) === 'missiond-blueprint');
  } catch (err) {
    return [finding({
      id: 'oversized-authoring-block',
      className: 'requires-user-decision',
      risk: 'low',
      summary: `V3 blueprint could not be parsed for authoring density analysis: ${err.message}`,
      evidenceRefs: [file],
      affectedSurfaces: ['nightly-evolution-loop'],
      recommendedChange: 'Repair Lisp syntax before running self-evolution density checks.',
      acceptance: ['node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp'],
      nonGoals: ['Do not infer architecture changes from an unparseable blueprint.'],
    })];
  }
  if (!root) return [];

  const lines = source.split(/\r?\n/);
  const rootLists = root.children.filter((child) => isList(child) && child.loc);
  const blockOffenders = [];
  for (let i = 0; i < rootLists.length; i += 1) {
    const child = rootLists[i];
    const id = blockName(child);
    const start = child.loc.line;
    const end = (rootLists[i + 1]?.loc?.line ?? lines.length + 1) - 1;
    const lineCount = Math.max(1, end - start + 1);
    if (lineCount > TOP_LEVEL_LINE_LIMIT) {
      blockOffenders.push({ id, start, lineCount });
    }
  }

  const noteOffenders = [];
  walk(root, (node) => {
    if (!isList(node)) return;
    const props = readKeywordProps(node);
    const note = props[':note']?.value;
    if (note?.type === 'string' && note.value.length > NOTE_CHAR_LIMIT) {
      noteOffenders.push({
        id: blockName(node),
        line: note.loc?.line ?? node.loc?.line ?? 1,
        chars: note.value.length,
      });
    }
  });

  if (blockOffenders.length === 0 && noteOffenders.length === 0) return [];
  const topBlocks = blockOffenders
    .sort((a, b) => b.lineCount - a.lineCount || a.id.localeCompare(b.id))
    .slice(0, 5);
  const topNotes = noteOffenders
    .sort((a, b) => b.chars - a.chars || a.id.localeCompare(b.id))
    .slice(0, 5);
  return [finding({
    id: 'oversized-authoring-block',
    className: 'architecture-proposal',
    risk: topBlocks.some((row) => row.lineCount > 500) || topNotes.some((row) => row.chars > 1800) ? 'medium' : 'low',
    summary: `V3 authoring density exceeds limits in ${blockOffenders.length} top-level block(s) and ${noteOffenders.length} long note(s).`,
    evidenceRefs: [
      ...topBlocks.map((row) => `${file}:${row.start} ${row.id} ${row.lineCount} lines`),
      ...topNotes.map((row) => `${file}:${row.line} ${row.id} note ${row.chars} chars`),
    ],
    affectedSurfaces: unique([
      ...topBlocks.map((row) => row.id),
      ...topNotes.map((row) => row.id),
    ]),
    recommendedChange: 'Move long explanatory evidence into sidecars and keep active V3 authoring blocks focused on executable entry/core/egress contracts.',
    acceptance: ['node scripts/check-v3-final-convergence.mjs --json --static-only', 'node scripts/check-lisp-blueprint-compression.mjs'],
    nonGoals: ['Do not delete evidence.', 'Do not compress runtime report directories or cold evidence by default.'],
  })];
}

function detectSurfaceFlowGaps(semantic) {
  const facts = semantic?.payload?.facts ?? [];
  const functionSurfaces = new Set(
    facts
      .filter((fact) => fact.kind === 'function' && fact.surface)
      .map((fact) => fact.surface),
  );
  const gaps = facts
    .filter((fact) => fact.kind === 'surface' && fact.id && !functionSurfaces.has(fact.id))
    .filter((fact) => !SURFACE_FLOW_GAP_ALLOWLIST.has(fact.id))
    .sort((a, b) => String(a.id).localeCompare(String(b.id)));
  if (gaps.length === 0) return [];
  return [finding({
    id: 'surface-flow-gap',
    className: 'safe-backfill',
    risk: 'low',
    summary: `${gaps.length} implementation surface(s) lack a matching semantic function in pillar-flow-map.`,
    evidenceRefs: gaps.map((gap) => `${gap.source?.source_file ?? BLUEPRINT}:${gap.source?.source_line ?? 1} surface ${gap.id}`),
    affectedSurfaces: gaps.map((gap) => gap.id),
    recommendedChange: 'Add or intentionally allowlist pillar-flow functions so every implementation surface has explicit entry/core/egress semantics.',
    acceptance: ['node scripts/check-v3-pillar-flow-schema.mjs', 'node scripts/check-v3-code-isomorphism-complete.mjs --json'],
    nonGoals: ['Do not infer function behavior from code automatically.', 'Do not mutate implementation-map from this analyzer.'],
  })];
}

function finding({
  id,
  className,
  risk,
  summary,
  evidenceRefs,
  affectedSurfaces,
  recommendedChange,
  acceptance,
  nonGoals,
}) {
  return {
    id,
    proposalId: null,
    class: className,
    risk,
    summary,
    evidenceRefs,
    affectedSurfaces,
    recommendedChange,
    acceptance,
    nonGoals,
    createdAt: null,
    nextAction: recommendedChange,
  };
}

function stableFindings(findings) {
  const riskRank = { low: 0, medium: 1, high: 2 };
  return [...findings].sort((a, b) => {
    const risk = (riskRank[a.risk] ?? 9) - (riskRank[b.risk] ?? 9);
    if (risk !== 0) return risk;
    return a.id.localeCompare(b.id);
  });
}

function walk(node, fn) {
  fn(node);
  if (!isList(node)) return;
  for (const child of node.children) walk(child, fn);
}

function blockName(node) {
  if (!isList(node)) return '<unknown>';
  const h = head(node) ?? '<list>';
  const second = nodeText(node.children[1]);
  return second && !second.startsWith(':') ? `${h}:${second}` : h;
}

function unique(values) {
  return [...new Set(values.filter(Boolean))].sort();
}

function tail(text, max) {
  return [...String(text)].slice(-max).join('');
}

function runFixtures() {
  const cases = [
    fixtureFinalConvergenceBlocker(),
    fixtureFacadeBudgetNearLimit(),
    fixtureOversizedAuthoringBlock(),
    fixtureGreenMinimal(),
  ];
  const failed = cases.filter((row) => !row.ok);
  if (failed.length > 0) {
    for (const row of failed) console.error(`fixture failed: ${row.name}: ${row.detail}`);
  }
  return { ok: failed.length === 0, cases: cases.length, results: cases };
}

function fixtureFinalConvergenceBlocker() {
  const findings = buildFindings(baseFixture({
    finalConvergence: { ok: false, failed_stage: 'fixture-stage', facades: [] },
  }));
  return assertFixture(
    'final-convergence-blocker',
    findings.some((finding) => finding.id === 'final-convergence-blocker'),
    'expected final-convergence-blocker finding',
  );
}

function fixtureFacadeBudgetNearLimit() {
  const findings = buildFindings(baseFixture({
    finalConvergence: {
      ok: true,
      facades: [{ id: 'mission_plan facade', file: 'plan.rs', lines: 80, maxLines: 100 }],
    },
  }));
  return assertFixture(
    'facade-budget-near-limit',
    findings.some((finding) => finding.id === 'facade-budget-near-limit'),
    'expected facade-budget-near-limit finding',
  );
}

function fixtureOversizedAuthoringBlock() {
  const longBlock = Array.from({ length: TOP_LEVEL_LINE_LIMIT + 2 }, (_, idx) => `      (step s${idx} :logic "x")`).join('\n');
  const source = `(missiond-blueprint\n  :schema "fixture"\n  (implementation-map\n${longBlock}\n  )\n)`;
  const findings = buildFindings(baseFixture({ blueprintSource: source }));
  return assertFixture(
    'oversized-authoring-block',
    findings.some((finding) => finding.id === 'oversized-authoring-block'),
    'expected oversized-authoring-block finding',
  );
}

function fixtureGreenMinimal() {
  const findings = buildFindings(baseFixture());
  return assertFixture(
    'green-minimal',
    findings.length === 0,
    `expected no findings, got ${findings.map((finding) => finding.id).join(',')}`,
  );
}

function baseFixture(overrides = {}) {
  const blueprintSource = overrides.blueprintSource ?? `(missiond-blueprint\n  :schema "fixture"\n  (pillar-flow-map\n    (pillar request\n      (function mission_request\n        :surface mission_request\n        :entry [mission_request]\n        :core ((step s1 :logic "ok"))\n        :egress [done])))\n  (implementation-map\n    (surface mission_request :status "code-aligned" :code ["request.rs"] :note "ok"))\n)`;
  const semantic = overrides.semantic ?? {
    payload: {
      facts: [
        { kind: 'function', id: 'mission_request', surface: 'mission_request' },
        { kind: 'surface', id: 'mission_request', source: { source_file: BLUEPRINT, source_line: 8 } },
      ],
    },
  };
  const workflows = overrides.workflows ?? { payload: { workflows: [{ workflow_id: 'nightly-evolution' }] } };
  const finalConvergence = overrides.finalConvergence ?? { ok: true, facades: [] };
  return { blueprintSource, semantic, workflows, finalConvergence, file: '<fixture>' };
}

function assertFixture(name, condition, detail) {
  return { name, ok: Boolean(condition), detail: condition ? null : detail };
}

main();
