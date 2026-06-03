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
  api: 'packages/board/src/api.ts',
  store: 'packages/board/src/store.ts',
  tasksRoute: 'packages/board/src/app/api/tasks/route.ts',
  boardClient: 'packages/board/src/lib/missiondBoardClient.ts',
  slotsRoute: 'packages/board/src/app/api/slots/route.ts',
  masterStatusRoute: 'packages/board/src/app/api/master/status/route.ts',
  projectsRoute: 'packages/board/src/app/api/projects/route.ts',
  taskDialog: 'packages/board/src/components/TaskDialog.tsx',
  terminal: 'packages/board/src/components/Terminal.tsx',
  systemDashboard: 'packages/board/src/components/SystemDashboard.tsx',
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
    ':fields [id label role running state provider engine modelProfile reasoningEffort searchEnabled sandbox approvalPolicy taskClass acceptsBoardTask mcpReady mcpEnabled mcpApprovalReady mcpApprovalMissingTools mcpSource providerConversationId confidence reason activeTool blockedKind currentTaskId activeBoardTaskId latestConversation]',
    'Codex master-control rows must surface missiond MCP readiness and MCP approval readiness',
    'Codex rows must prefer providerConversationId/latest real codex_cli conversation',
    ':state-contract',
    'Normalize PTY state case before classification',
    ':forbid [SLOT_OPTIONS hardcoded-sonnet-label stale-status-running]',
    '(projection pty-recognition',
    'Terminal labels must describe the selected provider/session generically',
    '(projection terminal-slot-selector',
    ':source [mission_slots mission_pty_status localStorage]',
    'Terminal slot selection lives inside the Terminal cockpit',
    '(frontend-runtime-config',
    ':generator "node scripts/project-frontend-board-config.mjs --write"',
    ':checker "node scripts/project-frontend-board-config.mjs --check"',
    ':output "packages/board/src/generated/board-frontend-config.ts"',
    '(board-task-api-contract',
    '(projection next-api-runtime-boundary',
    'Runtime-only MissionD proxy API routes must export nodejs runtime, force-dynamic, and revalidate=0',
    '(projection service-runtime-universe',
    ':source [mission_project.universe mission_project.deployment_channels service-runtime-universe deployment-channel-plane]',
    'SystemDashboard must show production service domain/deployment/DNS capability and per-project deploymentChannels',
    '(projection decision-inbox',
    ':source [mission_question DecisionDashboard JarvisChat]',
    'User-facing decisions live as durable mission_question records',
    '(event-routes',
    '(timeline-visuals',
  ]);

  requireText(diagnostics, FILES.types, src.types, [
    'export interface SlotDef',
    'provider?: string',
    'engine?: string',
    'modelProfile?: string',
    'acceptsBoardTask?: boolean',
    'mcpReady?: boolean',
    'mcpEnabled?: boolean',
    'mcpApprovalReady?: boolean',
    'mcpApprovalMissingTools?: string[]',
    'mcpSource?: string',
    'providerConversationId?: string',
    'currentTaskId?: string',
    'activeBoardTaskId?: string',
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
    'export const BOARD_TASK_API_ROUTE',
    'export const BOARD_TASK_FIELD_MAP',
    'export const BOARD_TASK_DEFAULTS',
    'export const BOARD_TASK_ACTIONS',
  ]);

  requireText(diagnostics, FILES.generator, src.generator, [
    "const BLUEPRINT = '.missiond/frontend/board-blueprint.lisp'",
    "const OUTPUT = 'packages/board/src/generated/board-frontend-config.ts'",
    'frontend-runtime-config',
    'EVENT_ROUTE_TABLE',
    'TIMELINE_EVENT_VISUALS',
    'BOARD_TASK_FIELD_MAP',
  ]);

  requireText(diagnostics, FILES.api, src.api, [
    'BOARD_TASK_API_ROUTE',
    'const BASE = BOARD_TASK_API_ROUTE',
  ]);

  requireText(diagnostics, FILES.store, src.store, [
    'BOARD_TASK_DEFAULTS',
    'BOARD_TASK_DEFAULTS.status',
    'BOARD_TASK_DEFAULTS.priority',
    'BOARD_TASK_DEFAULTS.category',
  ]);

  requireText(diagnostics, FILES.tasksRoute, src.tasksRoute, [
    'BOARD_TASK_FIELD_MAP',
    'mapToFrontend',
    'mapToBackend',
    'boardClient.create',
    'boardClient.update',
  ]);

  requireText(diagnostics, FILES.boardClient, src.boardClient, [
    'BOARD_TASK_ACTIONS',
    'callBoardTool',
    'callTool(BOARD_TOOL_BY_ACTION[action]',
    "'create'",
    "'update'",
  ]);

  requireText(diagnostics, FILES.slotsRoute, src.slotsRoute, [
    "callTool('mission_slots')",
    "callTool('mission_pty_status'",
    'provider',
    'engine',
    'modelProfile',
    'reasoningEffort',
    'searchEnabled',
    'approvalPolicy',
    'latestConversation',
    'acceptsBoardTask',
    'mcpReady',
    'mcpEnabled',
    'mcpApprovalReady',
    'mcpApprovalMissingTools',
    'mcpSource',
    'providerConversationId',
    'confidence',
  ]);

  requireText(diagnostics, FILES.masterStatusRoute, src.masterStatusRoute, [
    "export const runtime = 'nodejs'",
    "export const dynamic = 'force-dynamic'",
    'export const revalidate = 0',
    "callTool('mission_master_status')",
  ]);

  requireText(diagnostics, 'packages/board/src/components/SystemDashboard.tsx', src.systemDashboard ?? fs.readFileSync(path.join(repo, 'packages/board/src/components/SystemDashboard.tsx'), 'utf8'), [
    'DecisionDashboard',
    'runtimeServices',
    'deploymentChannels',
    'DeploymentChannelSummary',
    'GitHub Actions',
    'Native Runner',
    'ServiceRuntimeCard',
    'publicBaseUrl',
    'dnsProvider',
    'opsCapability',
  ]);

  requireText(diagnostics, FILES.projectsRoute, src.projectsRoute, [
    "action: 'universe'",
    "action: 'deployment_channels'",
    "kind: 'compiled'",
    'deploymentChannels: explicitChannels.length',
    'deploymentChannelDiagnostics',
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
    'mcpLabel',
    'MCP ready',
    'MCP approval',
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
    'slotStateLabel',
    'slotProviderGroup',
    'slotsByProvider',
    'Workstations',
    'Evidence',
    'Durable Conversation',
    'Diagnostics',
    'activeBoardTaskId',
    'latestConversation',
    'truncate',
  ]);
  if (/slots\.filter\(\(s\)\s*=>\s*s\.running\)\.map/.test(src.app)) {
    diagnostics.push({ file: FILES.app, message: 'Terminal slot selector must list all projected slots inside the panel, not only running slots in the global top bar' });
  }

  requireText(diagnostics, FILES.eventStream, src.eventStream, [
    'EVENT_ROUTE_TABLE',
    'EVENT_PREFIX_ROUTES',
    'EVENT_CUSTOM_EVENTS',
    'RESYNC_VERSION_KEYS',
    'dispatchConfiguredCustomEvent',
    'bumpKeys',
    'missiond.eventbus-live-lag-diagnostic.v1',
    'lastLagDiagnostic',
    'cursorLag',
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
    (projection service-runtime-universe :source [mission_project.universe mission_project.deployment_channels service-runtime-universe deployment-channel-plane] :rule "SystemDashboard must show production service domain/deployment/DNS capability and per-project deploymentChannels")
    (projection workstation-slots :source [mission_slots mission_pty_status workstation-pool] :fields [id label role running state provider engine modelProfile taskClass acceptsBoardTask confidence reason activeTool blockedKind latestConversation] :forbid [SLOT_OPTIONS hardcoded-sonnet-label])
    (projection pty-recognition :rule "Terminal labels must describe the selected provider/session generically")
    (projection terminal-slot-selector :source [mission_slots mission_pty_status localStorage] :rule "Terminal slot selection lives inside the Terminal panel"))
  (frontend-runtime-config :generator "node scripts/project-frontend-board-config.mjs --write" :checker "node scripts/project-frontend-board-config.mjs --check" :output "packages/board/src/generated/board-frontend-config.ts" (board-task-api-contract) (event-routes) (timeline-visuals)))`);
  fs.writeFileSync(path.join(root, FILES.types), 'export interface SlotDef { provider?: string; engine?: string; modelProfile?: string; acceptsBoardTask?: boolean; latestConversation?: { source?: string } }\n');
  fs.writeFileSync(path.join(root, FILES.constants), "export { CATEGORY_CONFIG, FLOW_PHASES, FLOW_TEMPLATE_OPTIONS } from './generated/board-frontend-config';\n");
  fs.writeFileSync(path.join(root, FILES.generated), 'GENERATED_FROM: .missiond/frontend/board-blueprint.lisp\nexport const BOARD_TABS = []; export const DEFAULT_TAB = "board"; export const TAB_MIGRATION = {}; export const EVENT_ROUTE_TABLE = []; export const EVENT_CUSTOM_EVENTS = []; export const EVENT_PREFIX_ROUTES = []; export const RESYNC_VERSION_KEYS = []; export const TIMELINE_EVENT_VISUALS = {}; export const TIMELINE_SLOT_COLORS = {}; export const TIMELINE_SWIMLANES = []; export const TIMELINE_WINDOW_OPTIONS = []; export const BOARD_TASK_API_ROUTE = "/api/tasks"; export const BOARD_TASK_FIELD_MAP = []; export const BOARD_TASK_DEFAULTS = {}; export const BOARD_TASK_ACTIONS = [];\n');
  fs.writeFileSync(path.join(root, FILES.generator), "const BLUEPRINT = '.missiond/frontend/board-blueprint.lisp'; const OUTPUT = 'packages/board/src/generated/board-frontend-config.ts'; frontend-runtime-config; EVENT_ROUTE_TABLE; TIMELINE_EVENT_VISUALS; BOARD_TASK_FIELD_MAP;\n");
  fs.writeFileSync(path.join(root, FILES.api), 'BOARD_TASK_API_ROUTE; const BASE = BOARD_TASK_API_ROUTE;\n');
  fs.writeFileSync(path.join(root, FILES.store), 'BOARD_TASK_DEFAULTS; BOARD_TASK_DEFAULTS.status; BOARD_TASK_DEFAULTS.priority; BOARD_TASK_DEFAULTS.category;\n');
  fs.writeFileSync(path.join(root, FILES.tasksRoute), "BOARD_TASK_FIELD_MAP; mapToFrontend; mapToBackend; callTool('mission_board_create'); callTool('mission_board_update');\n");
  fs.writeFileSync(path.join(root, FILES.slotsRoute), "callTool('mission_slots'); callTool('mission_pty_status'); provider; engine; modelProfile; latestConversation; acceptsBoardTask; confidence;\n");
  fs.writeFileSync(path.join(root, FILES.masterStatusRoute), "export const runtime = 'nodejs'; export const dynamic = 'force-dynamic'; export const revalidate = 0; callTool('mission_master_status');\n");
  fs.writeFileSync(path.join(root, FILES.projectsRoute), "callTool('mission_project', { action: 'universe' }); callTool('mission_project', { action: 'deployment_channels' }); kind: 'compiled'; deploymentChannels: explicitChannels.length; deploymentChannelDiagnostics;\n");
  fs.writeFileSync(path.join(root, FILES.taskDialog), "import type { SlotDef } from '../types'; fetch('/api/slots'); availableSlots; setAvailableSlots;\n");
  fs.writeFileSync(path.join(root, FILES.terminal), 'providerLabel; Starting session; No active session;\n');
  fs.writeFileSync(path.join(root, FILES.systemDashboard), 'DecisionDashboard; runtimeServices; deploymentChannels; DeploymentChannelSummary; GitHub Actions; Native Runner; ServiceRuntimeCard; publicBaseUrl; dnsProvider; opsCapability;\n');
  fs.writeFileSync(path.join(root, FILES.app), "import type { SlotDef } from './types'; BOARD_TABS; DEFAULT_TAB; TAB_MIGRATION; fetchSlots; /api/slots; slotStateLabel; overflow-x-auto overflow-y-hidden whitespace-nowrap; min-w-[160px] max-w-[280px]; truncate;\n");
  fs.writeFileSync(path.join(root, FILES.eventStream), 'EVENT_ROUTE_TABLE; EVENT_PREFIX_ROUTES; EVENT_CUSTOM_EVENTS; RESYNC_VERSION_KEYS; dispatchConfiguredCustomEvent; bumpKeys; missiond.eventbus-live-lag-diagnostic.v1; lastLagDiagnostic; cursorLag;\n');
  fs.writeFileSync(path.join(root, FILES.autopilotMonitor), "import { FLOW_PHASE_LABELS, FLOW_PHASES } from '../constants';\n");
  fs.writeFileSync(path.join(root, FILES.timelineConstants), "import { TIMELINE_EVENT_VISUALS, TIMELINE_SLOT_COLORS, TIMELINE_SWIMLANES } from '../../generated/board-frontend-config'; EVENT_ICON_MAP;\n");
  return root;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
