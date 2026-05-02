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

const BLUEPRINT = '.missiond/frontend/board-blueprint.lisp';
const V3_BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const CHECK_COMMAND = 'node scripts/check-frontend-board-lisp-schema.mjs';

const REQUIRED_PILLARS = [
  'app-shell',
  'missiond-proxy',
  'board-task-ui',
  'workstation-terminal-ui',
  'event-stream-ui',
  'timeline-log-ui',
  'knowledge-system-ui',
  'frontend-design-system',
];

const REQUIRED_SURFACES = [
  'app-shell',
  'missiond-proxy',
  'board-task-ui',
  'workstation-terminal-ui',
  'event-stream-ui',
  'timeline-log-ui',
  'knowledge-system-ui',
  'frontend-design-system',
];

const REQUIRED_CHECKS = [
  'node scripts/check-frontend-board-lisp-schema.mjs',
  'node scripts/check-frontend-board-code-isomorphism.mjs',
  'node scripts/check-frontend-board-runtime-projection.mjs',
  'node scripts/project-frontend-board-config.mjs --check',
];

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repo = opts.dryFixture ? buildFixture() : opts.repo;
  const result = checkRepo(repo, opts.blueprint, opts.v3Blueprint);
  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(`frontend board Lisp schema OK (${result.pillars.length} pillars, ${result.functions.length} functions)`);
  } else {
    for (const d of result.diagnostics) console.error(`${d.file}:${d.line}:${d.column}: ${d.message}`);
    console.error(`frontend board Lisp schema FAILED -- ${result.diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    dryFixture: false,
    repo: process.cwd(),
    blueprint: BLUEPRINT,
    v3Blueprint: V3_BLUEPRINT,
  };
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
      console.log(`Usage: node scripts/check-frontend-board-lisp-schema.mjs [--json] [--dry-fixture] [--repo <path>]`);
      process.exit(0);
    } else {
      fail(`unknown arg: ${arg}`);
    }
  }
  return opts;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

function checkRepo(repo, blueprintRel, v3Rel) {
  const diagnostics = [];
  const blueprintPath = path.join(repo, blueprintRel);
  const v3Path = path.join(repo, v3Rel);
  let source;
  let v3Source;
  try {
    source = fs.readFileSync(blueprintPath, 'utf8');
  } catch (err) {
    return result(false, [], [], [{ file: blueprintRel, line: 1, column: 1, message: `cannot read: ${err.message}` }]);
  }
  try {
    v3Source = fs.readFileSync(v3Path, 'utf8');
  } catch (err) {
    diagnostics.push(diag(v3Rel, null, `cannot read: ${err.message}`));
    v3Source = '';
  }

  const parsed = parseRoot(source, blueprintRel, diagnostics);
  if (!parsed.root) return result(false, [], [], diagnostics);
  const { root } = parsed;
  const rootProps = readKeywordProps(root, { start: 1 });
  requireProp(diagnostics, blueprintRel, rootProps, ':schema', 'missiond.frontend-blueprint.v1');
  requireProp(diagnostics, blueprintRel, rootProps, ':project', 'board');
  requireProp(diagnostics, blueprintRel, rootProps, ':root', 'packages/board');
  requireProp(diagnostics, blueprintRel, rootProps, ':authority', 'project-local-lisp-ssot');

  for (const section of ['project', 'runtime-projection', 'frontend-runtime-config', 'pillar-flow-map', 'implementation-map', 'workstation-dispatch-plan', 'quality-gates']) {
    if (!child(root, section)) diagnostics.push(diag(blueprintRel, root.loc, `missing (${section} ...) section`));
  }

  const flow = child(root, 'pillar-flow-map');
  const impl = child(root, 'implementation-map');
  const quality = child(root, 'quality-gates');
  const runtime = child(root, 'runtime-projection');
  const frontendRuntime = child(root, 'frontend-runtime-config');

  const pillars = [];
  const functions = [];
  const surfaceToFunctions = new Map();
  if (flow) {
    for (const pillar of flow.children.filter((n) => isList(n) && head(n) === 'pillar')) {
      const pillarId = nodeText(pillar.children[1]);
      if (pillarId) pillars.push(pillarId);
      const fnForms = pillar.children.filter((n) => isList(n) && head(n) === 'function');
      if (fnForms.length === 0) diagnostics.push(diag(blueprintRel, pillar.loc, `pillar "${pillarId}" must declare a function`));
      for (const fn of fnForms) {
        const fnId = nodeText(fn.children[1]);
        const props = readKeywordProps(fn, { start: 2 });
        const surface = nodeText(props[':surface']?.value);
        functions.push({ pillar: pillarId, function: fnId, surface });
        validateFunction(diagnostics, blueprintRel, fn, fnId, props);
        if (surface) {
          const refs = surfaceToFunctions.get(surface) ?? [];
          refs.push(`${pillarId}/${fnId}`);
          surfaceToFunctions.set(surface, refs);
        }
      }
    }
  }
  for (const pillar of REQUIRED_PILLARS) {
    if (!pillars.includes(pillar)) diagnostics.push(diag(blueprintRel, flow?.loc, `missing required pillar "${pillar}"`));
  }

  const surfaces = new Map();
  if (impl) {
    for (const surface of impl.children.filter((n) => isList(n) && head(n) === 'surface')) {
      const id = nodeText(surface.children[1]);
      if (!id) continue;
      const props = readKeywordProps(surface, { start: 2 });
      surfaces.set(id, { node: surface, props });
    }
  }
  for (const surface of REQUIRED_SURFACES) {
    const record = surfaces.get(surface);
    if (!record) {
      diagnostics.push(diag(blueprintRel, impl?.loc, `implementation-map missing surface "${surface}"`));
      continue;
    }
    requireProp(diagnostics, blueprintRel, record.props, ':status', 'code-aligned', record.node.loc);
    if (listTexts(record.props[':implements']?.value).length === 0) diagnostics.push(diag(blueprintRel, record.node.loc, `surface "${surface}" missing :implements`));
    if (listTexts(record.props[':code']?.value).length === 0) diagnostics.push(diag(blueprintRel, record.node.loc, `surface "${surface}" missing :code`));
    const refs = surfaceToFunctions.get(surface) ?? [];
    if (refs.length !== 1) diagnostics.push(diag(blueprintRel, flow?.loc, `surface "${surface}" must map to exactly one function, got ${refs.length}: ${refs.join(', ')}`));
  }

  if (runtime) {
    const workstation = runtime.children.find((n) => isList(n) && head(n) === 'projection' && nodeText(n.children[1]) === 'workstation-slots');
    if (!workstation) {
      diagnostics.push(diag(blueprintRel, runtime.loc, 'runtime-projection missing workstation-slots projection'));
    } else {
      const props = readKeywordProps(workstation, { start: 2 });
      requireListIncludes(diagnostics, blueprintRel, props, ':source', ['mission_slots', 'mission_pty_status', 'workstation-pool'], workstation.loc);
      requireListIncludes(diagnostics, blueprintRel, props, ':forbid', ['SLOT_OPTIONS', 'hardcoded-sonnet-label'], workstation.loc);
    }
  }

  if (frontendRuntime) validateFrontendRuntimeConfig(diagnostics, blueprintRel, frontendRuntime);

  if (quality) {
    const checks = listTexts(readKeywordProps(quality, { start: 1 })[':checks']?.value);
    for (const check of REQUIRED_CHECKS) {
      if (!checks.includes(check)) diagnostics.push(diag(blueprintRel, quality.loc, `quality-gates must include "${check}"`));
    }
  }

  for (const needle of [
    '(project-blueprint-registry',
    ':id board',
    ':path ".missiond/frontend/board-blueprint.lisp"',
    CHECK_COMMAND,
    'node scripts/check-frontend-board-code-isomorphism.mjs',
    'node scripts/check-frontend-board-runtime-projection.mjs',
    'scripts/project-frontend-board-config.mjs',
  ]) {
    if (!v3Source.includes(needle)) diagnostics.push(diag(v3Rel, null, `V3 project-blueprint-registry missing ${JSON.stringify(needle)}`));
  }

  return result(diagnostics.length === 0, pillars, functions, diagnostics);
}

function parseRoot(source, file, diagnostics) {
  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    diagnostics.push(diag(file, { line: err.line ?? 1, column: err.column ?? 1 }, err.message));
    return {};
  }
  const root = forms.find((form) => isList(form) && head(form) === 'missiond-frontend-blueprint');
  if (!root) diagnostics.push(diag(file, null, 'missing (missiond-frontend-blueprint ...) root'));
  return { root };
}

function validateFunction(diagnostics, file, fn, fnId, props) {
  if (!fnId) diagnostics.push(diag(file, fn.loc, 'function must declare an id'));
  if (listTexts(props[':entry']?.value).length === 0) diagnostics.push(diag(file, fn.loc, `function "${fnId}" missing non-empty :entry`));
  if (listTexts(props[':egress']?.value).length === 0) diagnostics.push(diag(file, fn.loc, `function "${fnId}" missing non-empty :egress`));
  const core = props[':core']?.value;
  if (!isList(core)) {
    diagnostics.push(diag(file, fn.loc, `function "${fnId}" missing :core ((step s1 ...) ...)`));
    return;
  }
  const steps = core.children.filter((n) => isList(n) && head(n) === 'step');
  if (steps.length === 0) diagnostics.push(diag(file, core.loc, `function "${fnId}" :core has no steps`));
  steps.forEach((step, index) => {
    const expected = `s${index + 1}`;
    const actual = nodeText(step.children[1]);
    if (actual !== expected) diagnostics.push(diag(file, step.loc, `function "${fnId}" core step ${index + 1} must be "${expected}", got ${JSON.stringify(actual)}`));
    const logic = nodeText(readKeywordProps(step, { start: 2 })[':logic']?.value);
    if (!logic || logic.trim() === '') diagnostics.push(diag(file, step.loc, `function "${fnId}" step "${actual ?? expected}" missing :logic`));
  });
}

function requireProp(diagnostics, file, props, key, expected, loc = null) {
  const actual = nodeText(props[key]?.value);
  if (actual !== expected) diagnostics.push(diag(file, loc ?? props[key]?.keyNode?.loc, `expected ${key} ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`));
}

function requireListIncludes(diagnostics, file, props, key, expected, loc) {
  const values = listTexts(props[key]?.value);
  for (const item of expected) {
    if (!values.includes(item)) diagnostics.push(diag(file, loc, `${key} must include ${JSON.stringify(item)}`));
  }
}

function validateFrontendRuntimeConfig(diagnostics, file, node) {
  const props = readKeywordProps(node, { start: 1 });
  requireProp(diagnostics, file, props, ':schema', 'missiond.frontend-runtime-config.v1', node.loc);
  requireProp(diagnostics, file, props, ':generator', 'node scripts/project-frontend-board-config.mjs --write', node.loc);
  requireProp(diagnostics, file, props, ':checker', 'node scripts/project-frontend-board-config.mjs --check', node.loc);
  requireProp(diagnostics, file, props, ':output', 'packages/board/src/generated/board-frontend-config.ts', node.loc);

  const tabs = child(node, 'tabs');
  const taxonomy = child(node, 'task-taxonomy');
  const flow = child(node, 'flow');
  const routes = child(node, 'event-routes');
  const timelineVisuals = child(node, 'timeline-visuals');
  for (const [name, section] of [['tabs', tabs], ['task-taxonomy', taxonomy], ['flow', flow], ['event-routes', routes], ['timeline-visuals', timelineVisuals]]) {
    if (!section) diagnostics.push(diag(file, node.loc, `frontend-runtime-config missing (${name} ...)`));
  }

  if (tabs) {
    const tabProps = readKeywordProps(tabs, { start: 1 });
    requireProp(diagnostics, file, tabProps, ':default', 'jarvis', tabs.loc);
    const ids = childrenByHead(tabs, 'tab').map((tab) => nodeText(readKeywordProps(tab, { start: 1 })[':id']?.value));
    for (const id of ['jarvis', 'board', 'terminal', 'exec', 'system', 'knowledge', 'logs']) {
      if (!ids.includes(id)) diagnostics.push(diag(file, tabs.loc, `frontend tabs missing ${id}`));
    }
    if (childrenByHead(tabs, 'migration').length === 0) diagnostics.push(diag(file, tabs.loc, 'frontend tabs must declare legacy migration entries'));
  }

  if (taxonomy) {
    for (const [kind, expected] of [['category', 8], ['priority', 3], ['group-option', 4], ['server-option', 4]]) {
      const count = childrenByHead(taxonomy, kind).length;
      if (count < expected) diagnostics.push(diag(file, taxonomy.loc, `task-taxonomy must declare at least ${expected} ${kind} entries, got ${count}`));
    }
  }

  if (flow) {
    if (childrenByHead(flow, 'template').length === 0) diagnostics.push(diag(file, flow.loc, 'flow must declare template entries'));
    const phases = childrenByHead(flow, 'phase').map((phase) => nodeText(readKeywordProps(phase, { start: 1 })[':id']?.value));
    for (const phase of ['investigate', 'plan', 'execute', 'finalize', 'done']) {
      if (!phases.includes(phase)) diagnostics.push(diag(file, flow.loc, `flow missing phase ${phase}`));
    }
  }

  if (routes) {
    const routeProps = readKeywordProps(routes, { start: 1 });
    requireListIncludes(diagnostics, file, routeProps, ':resync-bumps', ['slotVersion', 'taskVersion', 'engineVersion', 'timelineVersion'], routes.loc);
    if (childrenByHead(routes, 'route').length === 0) diagnostics.push(diag(file, routes.loc, 'event-routes must declare route entries'));
    for (const custom of ['briefing_summary_generated', 'jarvis_task_completed']) {
      const found = childrenByHead(routes, 'custom-event').some((event) => nodeText(readKeywordProps(event, { start: 1 })[':event']?.value) === custom);
      if (!found) diagnostics.push(diag(file, routes.loc, `event-routes missing custom-event ${custom}`));
    }
    const hasNarration = childrenByHead(routes, 'prefix-route').some((route) => nodeText(readKeywordProps(route, { start: 1 })[':prefix']?.value) === 'narration_');
    if (!hasNarration) diagnostics.push(diag(file, routes.loc, 'event-routes missing narration_ prefix route'));
  }

  if (timelineVisuals) {
    for (const [kind, expected] of [['event', 30], ['slot-color', 5], ['slot-fallback-line', 4], ['swimlane', 8], ['session-color', 8], ['window-option', 6]]) {
      const count = childrenByHead(timelineVisuals, kind).length;
      if (count < expected) diagnostics.push(diag(file, timelineVisuals.loc, `timeline-visuals must declare at least ${expected} ${kind} entries, got ${count}`));
    }
    for (const requiredType of ['slot_task_dispatched', 'board_task_updated', 'narration_failed']) {
      const found = childrenByHead(timelineVisuals, 'event').some((event) => nodeText(readKeywordProps(event, { start: 1 })[':type']?.value) === requiredType);
      if (!found) diagnostics.push(diag(file, timelineVisuals.loc, `timeline-visuals missing event ${requiredType}`));
    }
  }
}

function child(node, name) {
  return node.children.find((n) => isList(n) && head(n) === name);
}

function childrenByHead(node, name) {
  return node.children.filter((n) => isList(n) && head(n) === name);
}

function listTexts(node) {
  if (!node || !isList(node)) return [];
  return node.children.map((childNode) => nodeText(childNode)).filter((text) => text != null && text !== '');
}

function diag(file, loc, message) {
  return { file, line: loc?.line ?? 1, column: loc?.column ?? 1, message };
}

function result(ok, pillars, functions, diagnostics) {
  return { ok, pillars, functions, diagnostics };
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-frontend-board-schema-'));
  const blueprint = path.join(root, BLUEPRINT);
  const v3 = path.join(root, V3_BLUEPRINT);
  fs.mkdirSync(path.dirname(blueprint), { recursive: true });
  fs.mkdirSync(path.dirname(v3), { recursive: true });
  fs.writeFileSync(blueprint, `(missiond-frontend-blueprint
  :schema "missiond.frontend-blueprint.v1" :project board :root "packages/board" :authority "project-local-lisp-ssot"
  (project :id board)
  (runtime-projection
    (projection workstation-slots :source [mission_slots mission_pty_status workstation-pool] :forbid [SLOT_OPTIONS hardcoded-sonnet-label]))
  (frontend-runtime-config
    :schema "missiond.frontend-runtime-config.v1"
    :generator "node scripts/project-frontend-board-config.mjs --write"
    :checker "node scripts/project-frontend-board-config.mjs --check"
    :output "packages/board/src/generated/board-frontend-config.ts"
    (tabs :default jarvis
      (tab :id jarvis :label "Jarvis" :icon Sparkles)
      (tab :id board :label "Board" :icon ClipboardList)
      (tab :id terminal :label "Terminal" :icon MonitorUp)
      (tab :id exec :label "Exec" :icon Crosshair)
      (tab :id system :label "System" :icon Gauge)
      (tab :id knowledge :label "Knowledge" :icon Brain)
      (tab :id logs :label "Logs" :icon MessageSquareText)
      (migration :from old :to board))
    (task-taxonomy
      (category :id deploy :label "部署" :className "x")
      (category :id dev :label "开发" :className "x")
      (category :id infra :label "基建" :className "x")
      (category :id test :label "测试" :className "x")
      (category :id research :label "研究" :className "x")
      (category :id diagnosis :label "诊断" :className "x")
      (category :id investigation :label "调查" :className "x")
      (category :id other :label "其他" :className "x")
      (priority :id high :label "高" :dotColor "x")
      (priority :id medium :label "中" :dotColor "x")
      (priority :id low :label "低" :dotColor "x")
      (group-option :value none :label "不分组")
      (group-option :value category :label "按分类")
      (group-option :value priority :label "按优先级")
      (group-option :value project :label "按项目")
      (server-option :value "私有云")
      (server-option :value "ECS")
      (server-option :value "GCP")
      (server-option :value "Win Agent"))
    (flow
      (template :value "" :label "无")
      (phase :id investigate :label "调查")
      (phase :id plan :label "方案")
      (phase :id execute :label "执行")
      (phase :id finalize :label "收尾")
      (phase :id done :label "完成"))
    (event-routes
      :resync-bumps [slotVersion taskVersion engineVersion timelineVersion]
      (route :events [health_snapshot] :bump [engineVersion] :health-snapshot true)
      (custom-event :event briefing_summary_generated :name "timeline-summary-update" :detail [target_seq summary])
      (custom-event :event jarvis_task_completed :name "jarvis-task-completed" :detail [conversation_id task_id])
      (prefix-route :prefix "narration_" :bump [timelineVersion] :delay-ms 500))
    (timeline-visuals
      ${Array.from({ length: 30 }, (_, i) => `(event :type ${i === 0 ? 'slot_task_dispatched' : i === 1 ? 'board_task_updated' : i === 2 ? 'narration_failed' : `event_${i}`} :dot "d" :glow "g" :bg "b" :text "t" :label "l" :icon Activity)`).join('\n      ')}
      ${Array.from({ length: 5 }, (_, i) => `(slot-color :id slot-${i} :badge "b" :border "br" :line "l")`).join('\n      ')}
      ${Array.from({ length: 4 }, (_, i) => `(slot-fallback-line :value "line-${i}")`).join('\n      ')}
      ${Array.from({ length: 8 }, (_, i) => `(swimlane :id lane-${i} :label "Lane" :dot "d" :css "#fff" :bg "b" :types [event_${i}])`).join('\n      ')}
      ${Array.from({ length: 8 }, (_, i) => `(session-color :dot "d" :line "l" :ring "r")`).join('\n      ')}
      ${Array.from({ length: 6 }, (_, i) => `(window-option :label "${i}m" :value "${i}min")`).join('\n      ')}))
  (pillar-flow-map
    ${REQUIRED_PILLARS.map((pillar, i) => `(pillar ${pillar}
      (function fn-${i + 1} :surface ${REQUIRED_SURFACES[i]} :entry [in] :core ((step s1 :logic "do")) :egress [out]))`).join('\n    ')})
  (implementation-map
    ${REQUIRED_SURFACES.map((surface, i) => `(surface ${surface} :status "code-aligned" :implements [fn-${i + 1}] :code ["x"])`).join('\n    ')})
  (workstation-dispatch-plan :strategy two-stage-context-pack)
  (quality-gates :checks [${REQUIRED_CHECKS.map((c) => JSON.stringify(c)).join(' ')}]))`);
  fs.writeFileSync(v3, `(missiond-blueprint
  (project-blueprint-registry (project :id board :path ".missiond/frontend/board-blueprint.lisp" :checks ["${CHECK_COMMAND}" "node scripts/check-frontend-board-code-isomorphism.mjs" "node scripts/check-frontend-board-runtime-projection.mjs" "node scripts/project-frontend-board-config.mjs --check"] scripts/project-frontend-board-config.mjs)))`);
  return root;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
