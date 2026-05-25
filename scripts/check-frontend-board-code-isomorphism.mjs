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
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const BLUEPRINT = '.missiond/frontend/board-blueprint.lisp';
const V3_BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const CHECK_COMMAND = 'node scripts/check-frontend-board-code-isomorphism.mjs';

const REQUIRED_SURFACES = new Map([
  ['app-shell', ['packages/board/src/App.tsx', 'packages/board/src/generated/board-frontend-config.ts', 'packages/board/src/app/page.tsx', 'scripts/project-frontend-board-config.mjs']],
  ['missiond-proxy', ['packages/board/src/lib/missiond.ts', 'packages/board/src/app/api/tasks/route.ts', 'packages/board/src/app/api/slots/route.ts', 'packages/board/src/app/api/master/status/route.ts']],
  ['board-task-ui', ['packages/board/src/types.ts', 'packages/board/src/api.ts', 'packages/board/src/store.ts', 'packages/board/src/constants.ts', 'packages/board/src/generated/board-frontend-config.ts', 'packages/board/src/components/TaskDialog.tsx']],
  ['workstation-terminal-ui', ['packages/board/src/components/Terminal.tsx', 'packages/board/src/components/AutopilotMonitor.tsx', 'packages/board/src/app/api/pty/status/route.ts']],
  ['execution-cockpit-ui', ['packages/board/src/components/ExecDashboard.tsx', 'packages/board/src/components/Terminal.tsx', 'packages/board/src/App.tsx', 'packages/board/src/app/api/slots/route.ts', 'packages/board/src/eventStream.ts']],
  ['event-stream-ui', ['packages/board/src/eventStream.ts', 'packages/board/src/generated/board-frontend-config.ts', 'packages/board/src/hooks/useEventStream.ts']],
  ['timeline-log-ui', ['packages/board/src/components/timeline/CognitiveTimeline.tsx', 'packages/board/src/components/timeline/constants.tsx', 'packages/board/src/components/timeline/helpers.ts', 'packages/board/src/components/LogsConsolidated.tsx', 'packages/board/src/components/Conversations.tsx']],
  ['knowledge-system-ui', ['packages/board/src/components/KnowledgeConsolidated.tsx', 'packages/board/src/components/SystemDashboard.tsx', 'packages/board/src/components/DecisionDashboard.tsx', 'packages/board/src/components/architecture/ArchitectureView.tsx', 'packages/board/src/components/JarvisChat.tsx', 'packages/board/src/app/api/questions/route.ts']],
  ['frontend-design-system', ['packages/board/src/app/globals.css', 'packages/board/src/components/ui/button.tsx']],
]);

const REQUIRED_MAJOR_FILES = [
  'packages/board/src/App.tsx',
  'packages/board/src/types.ts',
  'packages/board/src/api.ts',
  'packages/board/src/store.ts',
  'packages/board/src/eventStream.ts',
  'packages/board/src/lib/missiond.ts',
  'packages/board/src/constants.ts',
  'packages/board/src/generated/board-frontend-config.ts',
  'packages/board/src/components/Terminal.tsx',
  'packages/board/src/components/TaskDialog.tsx',
  'packages/board/src/components/ExecDashboard.tsx',
  'packages/board/src/components/AutopilotMonitor.tsx',
  'packages/board/src/components/JarvisChat.tsx',
  'packages/board/src/components/DecisionDashboard.tsx',
  'packages/board/src/components/Conversations.tsx',
  'packages/board/src/components/timeline/CognitiveTimeline.tsx',
  'packages/board/src/components/timeline/constants.tsx',
  'packages/board/src/components/timeline/helpers.ts',
  'packages/board/src/app/api/slots/route.ts',
  'packages/board/src/app/api/master/status/route.ts',
  'packages/board/src/app/api/tasks/route.ts',
  'scripts/project-frontend-board-config.mjs',
];

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repo = opts.dryFixture ? buildFixture() : opts.repo;
  const result = checkRepo(repo, opts.blueprint, opts.v3Blueprint);
  if (opts.json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log(`frontend board code isomorphism OK (${result.surfaces.length} surfaces)`);
  else {
    for (const d of result.diagnostics) console.error(`${d.file}:${d.line}:${d.column}: ${d.message}`);
    console.error(`frontend board code isomorphism FAILED -- ${result.diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, dryFixture: false, repo: process.cwd(), blueprint: BLUEPRINT, v3Blueprint: V3_BLUEPRINT };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--dry-fixture') opts.dryFixture = true;
    else if (arg === '--repo') opts.repo = argv[++i] ?? fail('--repo requires a value');
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '--blueprint') opts.blueprint = argv[++i] ?? fail('--blueprint requires a value');
    else if (arg.startsWith('--blueprint=')) opts.blueprint = arg.slice('--blueprint='.length);
    else if (arg === '--v3-blueprint') opts.v3Blueprint = argv[++i] ?? fail('--v3-blueprint requires a value');
    else if (arg.startsWith('--v3-blueprint=')) opts.v3Blueprint = arg.slice('--v3-blueprint='.length);
    else if (arg === '-h' || arg === '--help') {
      console.log('Usage: node scripts/check-frontend-board-code-isomorphism.mjs [--json] [--dry-fixture]');
      process.exit(0);
    } else fail(`unknown arg: ${arg}`);
  }
  return opts;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

function checkRepo(repo, blueprintRel, v3Rel) {
  const diagnostics = [];
  const source = read(repo, blueprintRel, diagnostics);
  const v3 = read(repo, v3Rel, diagnostics);
  if (!source) return { ok: false, surfaces: [], diagnostics };
  const root = parseRoot(source, blueprintRel, diagnostics);
  if (!root) return { ok: false, surfaces: [], diagnostics };
  const impl = root.children.find((n) => isList(n) && head(n) === 'implementation-map');
  if (!impl) {
    diagnostics.push(diag(blueprintRel, root.loc, 'missing implementation-map'));
    return { ok: false, surfaces: [], diagnostics };
  }

  const surfaces = new Map();
  for (const node of impl.children.filter((n) => isList(n) && head(n) === 'surface')) {
    const id = nodeText(node.children[1]);
    if (!id) continue;
    const props = readKeywordProps(node, { start: 2 });
    surfaces.set(id, { node, props, code: listTexts(props[':code']?.value) });
  }

  const covered = new Set();
  for (const [surface, requiredFiles] of REQUIRED_SURFACES) {
    const record = surfaces.get(surface);
    if (!record) {
      diagnostics.push(diag(blueprintRel, impl.loc, `missing surface "${surface}"`));
      continue;
    }
    const status = nodeText(record.props[':status']?.value);
    if (status !== 'code-aligned') diagnostics.push(diag(blueprintRel, record.node.loc, `surface "${surface}" must be code-aligned, got ${JSON.stringify(status)}`));
    const implementsList = listTexts(record.props[':implements']?.value);
    if (implementsList.length === 0) diagnostics.push(diag(blueprintRel, record.node.loc, `surface "${surface}" missing :implements`));
    for (const required of requiredFiles) {
      if (!record.code.includes(required)) diagnostics.push(diag(blueprintRel, record.node.loc, `surface "${surface}" missing code ref ${required}`));
    }
    for (const rel of record.code) {
      covered.add(rel);
      if (!fs.existsSync(path.join(repo, rel))) diagnostics.push(diag(blueprintRel, record.node.loc, `code ref does not exist: ${rel}`));
    }
  }

  for (const rel of REQUIRED_MAJOR_FILES) {
    if (!covered.has(rel)) diagnostics.push(diag(blueprintRel, impl.loc, `major frontend file is not covered by any surface: ${rel}`));
  }

  const v3Needles = [
    '(surface board-frontend',
    ':implements [board-frontend]',
    ':code [".missiond/frontend/board-blueprint.lisp"',
    CHECK_COMMAND,
    'node scripts/check-frontend-board-lisp-schema.mjs',
    'node scripts/check-frontend-board-runtime-projection.mjs',
    'scripts/project-frontend-board-config.mjs',
    'packages/board/src/generated/board-frontend-config.ts',
  ];
  for (const needle of v3Needles) {
    if (!v3.includes(needle)) diagnostics.push(diag(v3Rel, null, `V3 board-frontend registry/surface missing ${JSON.stringify(needle)}`));
  }

  const execDashboardRel = 'packages/board/src/components/ExecDashboard.tsx';
  const execDashboard = read(repo, execDashboardRel, diagnostics);
  for (const needle of ['fetchTaskWithNotes', 'extractExecutionStepDigest', 'Execution Step Digest', 'interactionChainRows', 'Interaction Chain']) {
    if (!execDashboard.includes(needle)) {
      diagnostics.push(diag(execDashboardRel, null, `Exec cockpit missing execution-step-digest anchor ${JSON.stringify(needle)}`));
    }
  }
  for (const needle of ['execution-step-digest', 'PTY as a detail panel', 'runtime_metadata interaction chain']) {
    if (!source.includes(needle)) {
      diagnostics.push(diag(blueprintRel, null, `frontend blueprint missing execution cockpit intent ${JSON.stringify(needle)}`));
    }
  }

  return { ok: diagnostics.length === 0, surfaces: [...surfaces.keys()], diagnostics };
}

function read(repo, rel, diagnostics) {
  try {
    if (rel === V3_BLUEPRINT) return readBlueprintWithEvidenceSidecars(repo, rel);
    return fs.readFileSync(path.join(repo, rel), 'utf8');
  } catch (err) {
    diagnostics.push(diag(rel, null, `cannot read: ${err.message}`));
    return '';
  }
}

function parseRoot(source, file, diagnostics) {
  try {
    const forms = parseLisp(source, file);
    const root = forms.find((form) => isList(form) && head(form) === 'missiond-frontend-blueprint');
    if (!root) diagnostics.push(diag(file, null, 'missing missiond-frontend-blueprint root'));
    return root;
  } catch (err) {
    diagnostics.push(diag(file, { line: err.line ?? 1, column: err.column ?? 1 }, err.message));
    return null;
  }
}

function listTexts(node) {
  if (!node || !isList(node)) return [];
  return node.children.map((n) => nodeText(n)).filter((text) => text != null && text !== '');
}

function diag(file, loc, message) {
  return { file, line: loc?.line ?? 1, column: loc?.column ?? 1, message };
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-frontend-board-code-'));
  fs.mkdirSync(path.join(root, '.missiond/frontend'), { recursive: true });
  fs.mkdirSync(path.join(root, '.missiond/v3'), { recursive: true });
  const fixtureFiles = new Set(REQUIRED_MAJOR_FILES);
  for (const required of REQUIRED_SURFACES.values()) {
    for (const rel of required) fixtureFiles.add(rel);
  }
  for (const rel of fixtureFiles) {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    const body = rel === 'packages/board/src/components/ExecDashboard.tsx'
      ? '// fixture\nfetchTaskWithNotes\nextractExecutionStepDigest\nExecution Step Digest\ninteractionChainRows\nInteraction Chain\n'
      : '// fixture\n';
    fs.writeFileSync(path.join(root, rel), body);
  }
  const surfaceForms = [...REQUIRED_SURFACES.entries()].map(([surface, required]) => {
    const code = Array.from(new Set([...required, ...REQUIRED_MAJOR_FILES.filter((f) => surface === 'board-task-ui' && ['packages/board/src/constants.ts'].includes(f))]));
    return `(surface ${surface} :status "code-aligned" :implements [fn-${surface}] :code [${code.map((f) => JSON.stringify(f)).join(' ')}])`;
  });
  fs.writeFileSync(path.join(root, BLUEPRINT), `(missiond-frontend-blueprint
    (pillar execution-cockpit-ui
      (function execution-cockpit
        :core ((step s1 :logic "derive execution-step-digest and runtime_metadata interaction chain")
               (step s2 :logic "keep PTY as a detail panel"))))
    (implementation-map ${surfaceForms.join('\n')}))`);
  fs.writeFileSync(path.join(root, V3_BLUEPRINT), `(missiond-blueprint (implementation-map (surface board-frontend :implements [board-frontend] :code [".missiond/frontend/board-blueprint.lisp" "packages/board/src/generated/board-frontend-config.ts" "scripts/project-frontend-board-config.mjs" "node scripts/check-frontend-board-code-isomorphism.mjs" "node scripts/check-frontend-board-lisp-schema.mjs" "node scripts/check-frontend-board-runtime-projection.mjs"])))`);
  return root;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
