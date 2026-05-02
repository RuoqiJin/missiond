#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const BLUEPRINT = '.missiond/frontend/board-blueprint.lisp';

const FILES = {
  blueprint: BLUEPRINT,
  types: 'packages/board/src/types.ts',
  constants: 'packages/board/src/constants.ts',
  generated: 'packages/board/src/generated/board-frontend-config.ts',
  generator: 'scripts/project-frontend-board-config.mjs',
  slotsRoute: 'packages/board/src/app/api/slots/route.ts',
  taskDialog: 'packages/board/src/components/TaskDialog.tsx',
  terminal: 'packages/board/src/components/Terminal.tsx',
  app: 'packages/board/src/App.tsx',
  eventStream: 'packages/board/src/eventStream.ts',
  autopilotMonitor: 'packages/board/src/components/AutopilotMonitor.tsx',
  timelineConstants: 'packages/board/src/components/timeline/constants.tsx',
};

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repo = opts.dryFixture ? buildFixture() : opts.repo;
  const diagnostics = checkRepo(repo);
  const result = { ok: diagnostics.length === 0, diagnostics };
  if (opts.json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('frontend board runtime projection OK');
  else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`frontend board runtime projection FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, dryFixture: false, repo: process.cwd() };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--dry-fixture') opts.dryFixture = true;
    else if (arg === '--repo') opts.repo = argv[++i] ?? fail('--repo requires a value');
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '-h' || arg === '--help') {
      console.log('Usage: node scripts/check-frontend-board-runtime-projection.mjs [--json] [--dry-fixture]');
      process.exit(0);
    } else fail(`unknown arg: ${arg}`);
  }
  return opts;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

function checkRepo(repo) {
  const diagnostics = [];
  const src = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      src[key] = fs.readFileSync(path.join(repo, rel), 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireText(diagnostics, FILES.blueprint, src.blueprint, [
    '(projection workstation-slots',
    ':source [mission_slots mission_pty_status workstation-pool]',
    ':fields [id label role running state provider engine modelProfile taskClass acceptsBoardTask confidence reason activeTool blockedKind latestConversation]',
    ':forbid [SLOT_OPTIONS hardcoded-sonnet-label]',
    '(projection pty-recognition',
    'Terminal labels must describe the selected provider/session generically',
    '(frontend-runtime-config',
    ':generator "node scripts/project-frontend-board-config.mjs --write"',
    ':checker "node scripts/project-frontend-board-config.mjs --check"',
    ':output "packages/board/src/generated/board-frontend-config.ts"',
    '(event-routes',
    '(timeline-visuals',
  ]);

  requireText(diagnostics, FILES.types, src.types, [
    'export interface SlotDef',
    'provider?: string',
    'engine?: string',
    'modelProfile?: string',
    'acceptsBoardTask?: boolean',
    'latestConversation?:',
  ]);

  if (/export\s+const\s+SLOT_OPTIONS\b/.test(src.constants)) {
    diagnostics.push({ file: FILES.constants, message: 'SLOT_OPTIONS must not be a hardcoded frontend workstation pool; use /api/slots runtime projection' });
  }
  requireText(diagnostics, FILES.constants, src.constants, [
    "from './generated/board-frontend-config'",
    'CATEGORY_CONFIG',
    'FLOW_PHASES',
    'FLOW_TEMPLATE_OPTIONS',
  ]);

  requireText(diagnostics, FILES.generated, src.generated, [
    'GENERATED_FROM: .missiond/frontend/board-blueprint.lisp',
    'export const BOARD_TABS',
    'export const DEFAULT_TAB',
    'export const TAB_MIGRATION',
    'export const EVENT_ROUTE_TABLE',
    'export const EVENT_CUSTOM_EVENTS',
    'export const EVENT_PREFIX_ROUTES',
    'export const RESYNC_VERSION_KEYS',
    'export const TIMELINE_EVENT_VISUALS',
    'export const TIMELINE_SLOT_COLORS',
    'export const TIMELINE_SWIMLANES',
    'export const TIMELINE_WINDOW_OPTIONS',
  ]);

  requireText(diagnostics, FILES.generator, src.generator, [
    "const BLUEPRINT = '.missiond/frontend/board-blueprint.lisp'",
    "const OUTPUT = 'packages/board/src/generated/board-frontend-config.ts'",
    'frontend-runtime-config',
    'EVENT_ROUTE_TABLE',
    'TIMELINE_EVENT_VISUALS',
  ]);

  requireText(diagnostics, FILES.slotsRoute, src.slotsRoute, [
    "callTool('mission_slots')",
    "callTool('mission_pty_status'",
    'provider',
    'engine',
    'modelProfile',
    'latestConversation',
    'acceptsBoardTask',
    'confidence',
  ]);

  requireText(diagnostics, FILES.taskDialog, src.taskDialog, [
    "fetch('/api/slots')",
    'availableSlots',
    'SlotDef',
    'setAvailableSlots',
  ]);
  if (src.taskDialog.includes('SLOT_OPTIONS')) {
    diagnostics.push({ file: FILES.taskDialog, message: 'TaskDialog must use runtime slots from /api/slots, not SLOT_OPTIONS' });
  }

  if (/Claude Code/.test(src.terminal)) {
    diagnostics.push({ file: FILES.terminal, message: 'shared Terminal copy must be provider-neutral; found "Claude Code"' });
  }
  requireText(diagnostics, FILES.terminal, src.terminal, [
    'providerLabel',
    'Starting session',
    'No active session',
  ]);

  requireText(diagnostics, FILES.app, src.app, [
    "import type { SlotDef } from './types'",
    'BOARD_TABS',
    'DEFAULT_TAB',
    'TAB_MIGRATION',
    'fetchSlots',
    '/api/slots',
  ]);

  requireText(diagnostics, FILES.eventStream, src.eventStream, [
    'EVENT_ROUTE_TABLE',
    'EVENT_PREFIX_ROUTES',
    'EVENT_CUSTOM_EVENTS',
    'RESYNC_VERSION_KEYS',
    'dispatchConfiguredCustomEvent',
    'bumpKeys',
  ]);

  requireText(diagnostics, FILES.autopilotMonitor, src.autopilotMonitor, [
    "import { FLOW_PHASE_LABELS, FLOW_PHASES } from '../constants'",
  ]);

  requireText(diagnostics, FILES.timelineConstants, src.timelineConstants, [
    "from '../../generated/board-frontend-config'",
    'TIMELINE_EVENT_VISUALS',
    'TIMELINE_SLOT_COLORS',
    'TIMELINE_SWIMLANES',
    'EVENT_ICON_MAP',
  ]);

  return diagnostics;
}

function requireText(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push({ file, message: `missing required runtime projection text: ${needle}` });
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-frontend-runtime-projection-'));
  for (const rel of Object.values(FILES)) fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, FILES.blueprint), `(missiond-frontend-blueprint
  (runtime-projection
    (projection workstation-slots :source [mission_slots mission_pty_status workstation-pool] :fields [id label role running state provider engine modelProfile taskClass acceptsBoardTask confidence reason activeTool blockedKind latestConversation] :forbid [SLOT_OPTIONS hardcoded-sonnet-label])
    (projection pty-recognition :rule "Terminal labels must describe the selected provider/session generically"))
  (frontend-runtime-config :generator "node scripts/project-frontend-board-config.mjs --write" :checker "node scripts/project-frontend-board-config.mjs --check" :output "packages/board/src/generated/board-frontend-config.ts" (event-routes) (timeline-visuals)))`);
  fs.writeFileSync(path.join(root, FILES.types), 'export interface SlotDef { provider?: string; engine?: string; modelProfile?: string; acceptsBoardTask?: boolean; latestConversation?: { source?: string } }\n');
  fs.writeFileSync(path.join(root, FILES.constants), "export { CATEGORY_CONFIG, FLOW_PHASES, FLOW_TEMPLATE_OPTIONS } from './generated/board-frontend-config';\n");
  fs.writeFileSync(path.join(root, FILES.generated), 'GENERATED_FROM: .missiond/frontend/board-blueprint.lisp\nexport const BOARD_TABS = []; export const DEFAULT_TAB = "board"; export const TAB_MIGRATION = {}; export const EVENT_ROUTE_TABLE = []; export const EVENT_CUSTOM_EVENTS = []; export const EVENT_PREFIX_ROUTES = []; export const RESYNC_VERSION_KEYS = []; export const TIMELINE_EVENT_VISUALS = {}; export const TIMELINE_SLOT_COLORS = {}; export const TIMELINE_SWIMLANES = []; export const TIMELINE_WINDOW_OPTIONS = [];\n');
  fs.writeFileSync(path.join(root, FILES.generator), "const BLUEPRINT = '.missiond/frontend/board-blueprint.lisp'; const OUTPUT = 'packages/board/src/generated/board-frontend-config.ts'; frontend-runtime-config; EVENT_ROUTE_TABLE; TIMELINE_EVENT_VISUALS;\n");
  fs.writeFileSync(path.join(root, FILES.slotsRoute), "callTool('mission_slots'); callTool('mission_pty_status'); provider; engine; modelProfile; latestConversation; acceptsBoardTask; confidence;\n");
  fs.writeFileSync(path.join(root, FILES.taskDialog), "import type { SlotDef } from '../types'; fetch('/api/slots'); availableSlots; setAvailableSlots;\n");
  fs.writeFileSync(path.join(root, FILES.terminal), 'providerLabel; Starting session; No active session;\n');
  fs.writeFileSync(path.join(root, FILES.app), "import type { SlotDef } from './types'; BOARD_TABS; DEFAULT_TAB; TAB_MIGRATION; fetchSlots; /api/slots;\n");
  fs.writeFileSync(path.join(root, FILES.eventStream), 'EVENT_ROUTE_TABLE; EVENT_PREFIX_ROUTES; EVENT_CUSTOM_EVENTS; RESYNC_VERSION_KEYS; dispatchConfiguredCustomEvent; bumpKeys;\n');
  fs.writeFileSync(path.join(root, FILES.autopilotMonitor), "import { FLOW_PHASE_LABELS, FLOW_PHASES } from '../constants';\n");
  fs.writeFileSync(path.join(root, FILES.timelineConstants), "import { TIMELINE_EVENT_VISUALS, TIMELINE_SLOT_COLORS, TIMELINE_SWIMLANES } from '../../generated/board-frontend-config'; EVENT_ICON_MAP;\n");
  return root;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
