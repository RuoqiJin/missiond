#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { loadV3SemanticFacts, semanticFactDiagnostics } from './lib/v3_semantic_facts.mjs';

const CHECK_COMMAND = 'node scripts/check-v3-architecture-boundaries.mjs';

const REQUIRED_SOURCE_UNITS = [
  '.missiond/v3/missiond-blueprint.lisp',
  '.missiond/v3/shards/index.lisp',
  '.missiond/v3/shards/request-runtime.lisp',
  '.missiond/v3/shards/workstation-runtime.lisp',
  '.missiond/v3/shards/control-plane-runtime.lisp',
  '.missiond/v3/shards/memory-knowledge-runtime.lisp',
  '.missiond/v3/shards/implementation/request-surfaces.lisp',
  '.missiond/v3/shards/implementation/runtime-surfaces.lisp',
  '.missiond/v3/shards/implementation/knowledge-surfaces.lisp',
  '.missiond/v3/shards/implementation/ops-surfaces.lisp',
];

const REQUIRED_SURFACES = [
  'mission_request',
  'mission_board',
  'autopilot-runtime',
  'workstation-config',
  'workstation-dispatch',
  'eventbridge',
  'memory-provider-boundary',
  'mission-shared-memory',
  'board-frontend',
  'typed-lisp-compiler',
];

const REQUIRED_FILES = {
  appPorts: 'crates/missiond-daemon/src/app_ports.rs',
  runtimeActors: 'crates/missiond-daemon/src/runtime_actors.rs',
  autopilotWorkflow: 'crates/missiond-daemon/src/engine/intent_engine/autopilot_workflow.rs',
  toolCatalog: 'crates/missiond-mcp/src/tools/mod.rs',
  boardClient: 'packages/board/src/lib/missiondBoardClient.ts',
  domainCrate: 'crates/missiond-domain/src/lib.rs',
};

function main() {
  const json = process.argv.includes('--json');
  const repo = process.cwd();
  const diagnostics = [];
  const facts = loadV3SemanticFacts({ repoRoot: repo });
  diagnostics.push(...semanticFactDiagnostics(facts));

  if (diagnostics.length === 0) {
    checkSourceUnits(repo, facts, diagnostics);
    checkCheckerRegistry(facts, diagnostics);
    checkRequiredSurfaces(repo, facts, diagnostics);
    checkGeneratedAbi(repo, diagnostics);
    checkEventBusBoundary(repo, diagnostics);
    checkImplementationScaffolds(repo, diagnostics);
    checkPlaneMigrationUsage(repo, diagnostics);
    checkToolAndBoardSchema(repo, diagnostics);
    checkCrateSplitBoundary(repo, diagnostics);
  }

  const result = {
    ok: diagnostics.length === 0,
    sourceUnits: facts.sourceUnits?.size ?? 0,
    surfaces: facts.surfaces?.size ?? 0,
    diagnostics,
  };
  if (json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('v3 architecture boundary check OK');
  else for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
  process.exit(result.ok ? 0 : 1);
}

function checkSourceUnits(repo, facts, diagnostics) {
  const units = [...facts.sourceUnits.values()];
  const byRel = new Set(units.map((unit) => repoRelative(repo, unit.file)));
  for (const rel of REQUIRED_SOURCE_UNITS) {
    if (!byRel.has(rel)) {
      diagnostics.push(diag(rel, `compiled source_units missing ${rel}`));
    }
  }
  for (const unit of units) {
    const rel = repoRelative(repo, unit.file);
    if (!rel.startsWith('.missiond/v3/')) {
      diagnostics.push(diag(rel, `compiled source unit must stay under .missiond/v3: ${rel}`));
      continue;
    }
    if (!fs.existsSync(path.join(repo, rel))) {
      diagnostics.push(diag(rel, `compiled source unit does not exist: ${rel}`));
    }
  }
}

function checkCheckerRegistry(facts, diagnostics) {
  const checker = facts.checkerRegistry.get('v3-compression-contract');
  if (!checker) {
    diagnostics.push(diag('.missiond/v3/missiond-blueprint.lisp', 'compiled checker registry missing v3-compression-contract'));
    return;
  }
  if (!checker.checks.includes(CHECK_COMMAND)) {
    diagnostics.push(diag('.missiond/v3/missiond-blueprint.lisp', `compression-contract must include ${CHECK_COMMAND}`));
  }
}

function checkRequiredSurfaces(repo, facts, diagnostics) {
  for (const id of REQUIRED_SURFACES) {
    const surface = facts.surfaces.get(id);
    if (!surface) {
      diagnostics.push(diag('.missiond/v3/missiond-blueprint.lisp', `compiled surface missing ${id}`));
      continue;
    }
    if (surface.status !== 'code-aligned') {
      diagnostics.push(diag(surfaceFile(surface), `surface ${id} must be :status "code-aligned"`));
    }
    if (!Array.isArray(surface.code) || surface.code.length === 0) {
      diagnostics.push(diag(surfaceFile(surface), `surface ${id} must declare implementation :code paths`));
    }
    if (typeof surface.note !== 'string' || surface.note.trim() === '') {
      diagnostics.push(diag(surfaceFile(surface), `surface ${id} must declare a non-empty :note`));
    }
    const repoPaths = (surface.code ?? [])
      .filter((codePath) => typeof codePath === 'string')
      .filter((codePath) => !path.isAbsolute(codePath))
      .map((codePath) => codePath.split(':')[0]);
    if (!repoPaths.some((codePath) => fs.existsSync(path.join(repo, codePath)))) {
      diagnostics.push(diag(surfaceFile(surface), `surface ${id} must cite at least one existing repo-local implementation path`));
    }
  }
}

function checkGeneratedAbi(repo, diagnostics) {
  const proc = spawnSync(process.execPath, ['scripts/project-v3-contracts.mjs', '--check', '--json'], {
    cwd: repo,
    encoding: 'utf8',
    timeout: 60_000,
    maxBuffer: 1024 * 1024 * 8,
  });
  if (proc.status !== 0 || proc.error) {
    diagnostics.push(diag('scripts/project-v3-contracts.mjs', proc.error?.message ?? proc.stderr?.trim() ?? 'generated V3 ABI check failed'));
    return;
  }
  let payload;
  try {
    payload = JSON.parse(proc.stdout);
  } catch (err) {
    diagnostics.push(diag('scripts/project-v3-contracts.mjs', `generated V3 ABI check returned invalid JSON: ${err.message}`));
    return;
  }
  if (payload.ok !== true) {
    diagnostics.push(diag('scripts/project-v3-contracts.mjs', `generated V3 ABI is stale: ${JSON.stringify(payload.diagnostics ?? [])}`));
  }
}

function checkEventBusBoundary(repo, diagnostics) {
  const bootstrap = read(repo, 'crates/missiond-daemon/src/bus/bootstrap.rs', diagnostics);
  const wsBridge = read(repo, 'crates/missiond-daemon/src/bus/ws_bridge.rs', diagnostics);
  const subscribers = read(repo, 'crates/missiond-daemon/src/bus/v2_subscribers.rs', diagnostics);
  requireAll('crates/missiond-daemon/src/bus/bootstrap.rs', bootstrap, diagnostics, [
    'pub struct BusServices',
    'pub async fn publish_board',
    'pub async fn publish<E: DomainEvent>',
    'self.log.append(event, opts).await',
  ]);
  requireAll('crates/missiond-daemon/src/bus/ws_bridge.rs', wsBridge, diagnostics, [
    'event_log',
    'v1-compatible',
    'v2_logged_to_v1_wire_format',
  ]);
  if (/state\.pty\.send\s*\(/.test(subscribers)) {
    diagnostics.push(diag('crates/missiond-daemon/src/bus/v2_subscribers.rs', 'v2 subscribers must notify/ack only; PTY send belongs to Autopilot dispatch ownership'));
  }

  const directAppends = grepFiles(path.join(repo, 'crates/missiond-daemon/src'), /\.log\.append\s*\(/)
    .map((file) => repoRelative(repo, file))
    .filter((rel) => rel !== 'crates/missiond-daemon/src/bus/bootstrap.rs');
  for (const rel of directAppends) {
    diagnostics.push(diag(rel, 'daemon event producers must publish through BusServices, not append to the log directly'));
  }
}

function checkImplementationScaffolds(repo, diagnostics) {
  requireFileAnchors(repo, REQUIRED_FILES.appPorts, diagnostics, [
    'trait BoardTaskRepo',
    'trait SlotLeaseRepo',
    'trait ConversationFinalRepo',
    'trait EventLogRepo',
    'trait BoardEventPublisher',
    'struct StorePorts',
  ]);
  requireFileAnchors(repo, REQUIRED_FILES.runtimeActors, diagnostics, [
    'enum SlotActorCommand',
    'enum ExtractionLaneCommand',
    'struct SlotActorHandle',
    'struct ExtractionLaneHandle',
    'struct SlotActorSnapshot',
    'struct ExtractionLaneSnapshot',
    'fn from_state',
  ]);
  requireFileAnchors(repo, REQUIRED_FILES.autopilotWorkflow, diagnostics, [
    'struct AutopilotScheduler',
    'struct BoardTaskDispatcher',
    'struct DispatchRunActor',
    'struct MaintenanceRunner',
    'failure_policy_for_send_error',
    'TaskResultArtifact',
  ]);
  requireFileAnchors(repo, 'crates/missiond-daemon/src/state.rs', diagnostics, [
    'struct StoragePlane',
    'struct EventPlane',
    'fn storage_plane(&self)',
    'fn event_plane(&self)',
    'fn slot_actor(&self',
    'fn fast_extraction_lane(&self)',
    'fn slow_extraction_lane(&self)',
  ]);
  requireFileAnchors(repo, 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs', diagnostics, [
    'DispatchRunActor::new',
    'DispatchRunActor::failure_policy_for_send_error',
  ]);
  requireFileAnchors(repo, 'crates/missiond-daemon/src/handlers/sysinfra/misc.rs', diagnostics, [
    'state.slot_actor',
    'slotActors',
    'fast_extraction_lane().snapshot()',
    'slow_extraction_lane().snapshot()',
  ]);
}

function checkPlaneMigrationUsage(repo, diagnostics) {
  const migratedBoardHandlers = [
    'crates/missiond-daemon/src/handlers/knowledge/board/create.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/update.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/query.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/note.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/claim.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/delete.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/retry.rs',
    'crates/missiond-daemon/src/handlers/knowledge/board/events.rs',
  ];
  for (const rel of migratedBoardHandlers) {
    const source = read(repo, rel, diagnostics);
    if (/state\.(store|bus)\b/.test(source)) {
      diagnostics.push(diag(rel, 'migrated Board handler must use storage/event planes instead of direct AppState store/bus fields'));
    }
  }
  requireFileAnchors(repo, 'crates/missiond-daemon/src/handlers/knowledge/board/create.rs', diagnostics, ['state.storage_plane()']);
  requireFileAnchors(repo, 'crates/missiond-daemon/src/handlers/knowledge/board/events.rs', diagnostics, ['state.event_plane().bus']);
}

function checkToolAndBoardSchema(repo, diagnostics) {
  requireFileAnchors(repo, REQUIRED_FILES.toolCatalog, diagnostics, [
    'struct ToolCatalogEntry',
    'pub fn tool_catalog',
    'pub fn legacy_tool_aliases',
    'primary_tool_family',
  ]);
  requireFileAnchors(repo, REQUIRED_FILES.boardClient, diagnostics, [
    'BOARD_TASK_ACTIONS',
    'callBoardTool',
    'boardClient',
  ]);
  requireFileAnchors(repo, 'packages/board/src/app/api/tasks/route.ts', diagnostics, [
    'boardClient',
    'mapToFrontend',
    'mapToBackend',
  ]);
  const directBoardToolCalls = grepSourceFiles(path.join(repo, 'packages/board/src/app/api'), /callTool\s*\(\s*['"`]mission_board_/, ['.ts', '.tsx'])
    .map((file) => repoRelative(repo, file));
  for (const rel of directBoardToolCalls) {
    diagnostics.push(diag(rel, 'Board API routes must use generated typed boardClient instead of direct mission_board_* callTool'));
  }
}

function checkCrateSplitBoundary(repo, diagnostics) {
  requireFileAnchors(repo, 'Cargo.toml', diagnostics, [
    '"crates/missiond-domain"',
    'missiond-domain = { version = "0.1.0", path = "crates/missiond-domain" }',
  ]);
  requireFileAnchors(repo, 'crates/missiond-daemon/Cargo.toml', diagnostics, ['missiond-domain = { workspace = true }']);
  requireFileAnchors(repo, REQUIRED_FILES.domainCrate, diagnostics, [
    'pub mod architecture',
    'pub mod ids',
    'impl_transparent_id',
    'MISSIOND_CORE_COMPAT_FACADE',
  ]);
  requireFileAnchors(repo, REQUIRED_FILES.autopilotWorkflow, diagnostics, ['missiond_domain::ids']);
}

function requireFileAnchors(repo, rel, diagnostics, anchors) {
  const source = read(repo, rel, diagnostics);
  requireAll(rel, source, diagnostics, anchors);
}

function requireAll(file, source, diagnostics, anchors) {
  for (const anchor of anchors) {
    if (!source.includes(anchor)) {
      diagnostics.push(diag(file, `missing architecture boundary anchor: ${anchor}`));
    }
  }
}

function read(repo, rel, diagnostics) {
  try {
    return fs.readFileSync(path.join(repo, rel), 'utf8');
  } catch (err) {
    diagnostics.push(diag(rel, `cannot read: ${err.message}`));
    return '';
  }
}

function grepFiles(root, re) {
  return grepSourceFiles(root, re, ['.rs']);
}

function grepSourceFiles(root, re, exts) {
  const out = [];
  for (const file of walk(root)) {
    if (!exts.some((ext) => file.endsWith(ext))) continue;
    const source = fs.readFileSync(file, 'utf8');
    if (re.test(source)) out.push(file);
  }
  return out;
}

function walk(root) {
  const out = [];
  if (!fs.existsSync(root)) return out;
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const abs = path.join(root, entry.name);
    if (entry.isDirectory()) out.push(...walk(abs));
    else out.push(abs);
  }
  return out;
}

function surfaceFile(surface) {
  return surface?.source?.source_file ?? surface?.source?.file ?? '.missiond/v3/missiond-blueprint.lisp';
}

function repoRelative(repo, maybeAbs) {
  if (!maybeAbs) return '';
  if (!path.isAbsolute(maybeAbs)) return maybeAbs.replaceAll('\\', '/');
  return path.relative(repo, maybeAbs).replaceAll('\\', '/');
}

function diag(file, message) {
  return { file, line: 1, column: 1, code: 'V3_ARCHITECTURE_BOUNDARY', message };
}

main();
