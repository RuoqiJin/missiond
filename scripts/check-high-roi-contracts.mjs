#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-high-roi-contracts.mjs [--json]

Checks high-ROI MissionD repair contracts:
- source-state labels are centralized in missiond-core constants
- operator overview reports partial/errors diagnostics
- Board Event Health exposes stale-derived status
- mission_health exposes startupPreflight
- worker lifecycle and evidence/event health operator contracts stay wired
- executable runbook actions, operator trends, and real bus metrics stay wired
- completion evidence writer, workflow_run adoption, compiled ABI boundary,
  workstation exact-shard validation, and rollback-smoke deploy discipline stay wired
`;

const SOURCE_STATE_LABELS = [
  'provider-index-missing',
  'missing-stale',
  'path-mismatch',
  'codex_local_index',
  'sqlite-missing',
];

const SOURCE_LABEL_ALLOW = new Set([
  'crates/missiond-core/src/types/conversation.rs',
]);

function main() {
  const args = process.argv.slice(2);
  const json = args.includes('--json');
  if (args.some((arg) => arg !== '--json' && arg !== '--help' && arg !== '-h')) {
    console.error(usage);
    process.exit(2);
  }
  if (args.includes('--help') || args.includes('-h')) {
    console.log(usage);
    process.exit(0);
  }

  const root = process.cwd();
  const diagnostics = [
    ...checkSourceStateConstants(root),
    ...checkRequiredText(root, 'packages/board/src/lib/operatorOverview.ts', [
      ['partial', /partial:\s*errors\.length\s*>\s*0/],
      ['errors array', /errors:\s*OverviewError\[\]/],
      ['error source', /source:\s*['"]slotStatus['"]/],
      ['workers overview', /mission_worker[\s\S]*action:\s*['"]list['"]/],
      ['runbook generator', /buildRunbook/],
      ['runbook executable action', /action:\s*\{/],
      ['operator trends projection', /operatorTrends/],
      ['workflow runs projection', /workflowRuns/],
      ['workflow blocked runbook action', /workflow_blocked_inspect/],
      ['workflow resume runbook action', /workflow_resume:/],
      ['fake MCP harness', /createFakeOperatorOverviewHarness/],
    ]),
    ...checkRequiredText(root, 'packages/board/src/app/api/operator/runbook/action/route.ts', [
      ['runbook action route', /export\s+async\s+function\s+POST/],
      ['runbook action allowlist', /ALLOWED_TOOLS/],
      ['DLQ replay confirm gate', /dlq_replay[\s\S]*confirm=true|confirm.*dlq_replay/s],
      ['workflow resume confirm gate', /workflow_resume[\s\S]*confirm=true|confirm.*workflow_resume/s],
    ]),
    ...checkRequiredText(root, 'packages/board/src/eventStream.ts', [
      ['stale threshold', /EVENT_HEALTH_STALE_AFTER_MS/],
      ['stale flag', /eventHealthIsStale/],
      ['derived status', /eventHealthStatus/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/handlers/sysinfra/misc.rs', [
      ['startupPreflight health field', /startupPreflight/],
      ['eventBus health field', /eventBus/],
      ['workers health field', /workers/],
      ['evidence health field', /evidence/],
      ['operator trends health field', /operatorTrends/],
      ['workflow runs health field', /workflowRuns/],
      ['mission_event_bus tool', /mission_event_bus/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/workers/registry.rs', [
      ['worker lifecycle enum', /enum\s+WorkerLifecycleState/],
      ['worker health snapshot', /struct\s+WorkerHealthSnapshot/],
      ['worker heartbeat method', /pub\s+fn\s+heartbeat/],
      ['worker poll lifecycle helper', /pub\s+fn\s+begin_poll/],
      ['worker event lifecycle helper', /pub\s+fn\s+begin_event/],
      ['worker stale reason', /stale_reason/],
    ]),
    ...checkWorkerLifecycleUse(root),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/bus/bootstrap.rs', [
      ['bus health snapshot', /health_snapshot/],
      ['DLQ health', /dead_letter_queue/],
      ['ws bridge health', /wsBridge/],
      ['operator trends store', /operator_health_samples/],
      ['workflow trend counters', /workflow_blocked[\s\S]*workflow_failed[\s\S]*workflow_stale/],
      ['DLQ action list', /dlq_list/],
      ['DLQ action replay', /dlq_replay/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/engine/task_completion_evidence.rs', [
      ['completion evidence writer', /struct\s+TaskCompletionEvidenceWriter/],
      ['evidence write failed code', /EVIDENCE_WRITE_FAILED/],
      ['evidence write timeout code', /EVIDENCE_WRITE_TIMEOUT/],
      ['shared memory task result put', /task_result_put/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs', [
      ['mission_execution completion writes evidence', /TaskCompletionEvidenceWriter[\s\S]*task_result_artifact/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs', [
      ['plan execute contract required', /PLAN_CONTRACT_REQUIRED/],
      ['plan dag workflow start', /workflow_start[\s\S]*mission_plan_execute/],
      ['plan dag workflow checkpoint', /workflow_checkpoint[\s\S]*aggregate_status/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs', [
      ['swarm workflow start', /workflow_start[\s\S]*mission_swarm_run/],
      ['swarm workflow checkpoint', /workflow_checkpoint[\s\S]*created_task_ids/],
      ['exact shard ready override', /exact_shard_ready/],
      ['implementation exact shard requirement', /mission_task_delegate refused an implementation worker without write_scope/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/app_ports.rs', [
      ['task evidence port', /trait\s+TaskEvidencePort/],
      ['workflow run port', /trait\s+WorkflowRunPort/],
      ['board completion port', /trait\s+BoardCompletionPort/],
      ['slot status port', /trait\s+SlotStatusPort/],
    ]),
    ...checkRequiredText(root, 'scripts/deploy-daemon.sh', [
      ['default mcp config JSON validation', /validate_default_mcp_config/],
      ['rollback smoke timing', /rollback-smoke/],
      ['rollback smoke wrapper', /rollback_with_smoke/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs', [
      ['log writer metrics field', /metrics:\s*Arc<dyn BusMetrics>/],
      ['log writer records append', /record_append/],
      ['metrics spawn constructor', /spawn_log_writer_with_metrics/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-core/src/event/pipeline/step3_commit/handle.rs', [
      ['append handle records reject', /record_reject/],
      ['append handle records ephemeral', /opts\.ephemeral[\s\S]*record_append/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-core/src/event/pipeline/step5_tail/dispatcher.rs', [
      ['dispatcher metrics argument', /bus_metrics:\s*Arc<dyn BusMetrics>/],
      ['dispatcher dropped metric', /record_control_gate_dropped/],
      ['dispatcher topic depth metric', /record_topic_depth/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-daemon/src/engine/shared_memory.rs', [
      ['task evidence summary schema', /missiond\.task-evidence-summary\.v1/],
      ['task evidence summary action', /task_evidence_summary/],
      ['task evidence gate summary', /requiredForDone/],
      ['workflow runs summary', /workflow_runs_summary/],
      ['workflow startup recovery', /startup_recover_workflow_runs/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-core/src/db/pg/board.rs', [
      ['evidence hard gate', /EVIDENCE_REQUIRED/],
      ['task result artifact gate query', /task_result_artifacts[\s\S]*result_status/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-core/migrations/20260526000000_worker_runtime_state.sql', [
      ['worker runtime table', /CREATE TABLE IF NOT EXISTS worker_runtime_state/],
      ['worker runtime lifecycle', /lifecycle TEXT NOT NULL/],
    ]),
    ...checkRequiredText(root, 'crates/missiond-core/migrations/20260526001000_operator_health_samples.sql', [
      ['operator health samples table', /CREATE TABLE IF NOT EXISTS operator_health_samples/],
      ['evidence completion partial index', /idx_task_result_artifacts_completed_gate/],
    ]),
    ...checkRequiredText(root, 'scripts/check-board-operator-fixtures.mjs', [
      ['board operator fixture checker', /createFakeOperatorOverviewHarness/],
      ['runbook action assertion', /worker_resume:/],
      ['workflow runbook assertion', /workflow_blocked_inspect/],
    ]),
    ...checkRequiredText(root, 'scripts/check-pg-migrations-discipline.mjs', [
      ['pg migration checker', /check-pg-migrations-discipline/],
      ['destructive migration marker', /missiond-allow-destructive-migration/],
      ['frozen migration checksum map', /FROZEN_SHA384/],
      ['frozen init migration checksum', /cdb48608a9cdfe380c0e00078072195e0a02213e4addecf23643ad40730f8619ed3ceee184e3cc4806a1e8a614e429c9/],
    ]),
    ...checkForbiddenText(root, 'crates/missiond-daemon/src/handlers/compute/worker.rs', [
      ['direct worker registry access', /state\.worker_registry/],
      ['direct control manager access', /state\.control_manager/],
      ['direct pty access', /state\.pty/],
      ['direct mission access', /state\.mission/],
    ]),
    ...checkForbiddenText(root, 'crates/missiond-daemon/src/handlers/sysinfra/misc.rs', [
      ['direct store access', /state\.store/],
      ['direct control manager access', /state\.control_manager/],
      ['direct mission access', /state\.mission/],
    ]),
  ];

  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('MissionD high-ROI contract check OK');
  } else {
    for (const diagnostic of diagnostics) {
      console.error(`${diagnostic.file}:${diagnostic.line}: ${diagnostic.message}`);
    }
    console.error(`MissionD high-ROI contract check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function checkWorkerLifecycleUse(root) {
  const workerRoot = path.join(root, 'crates/missiond-daemon/src/workers');
  const diagnostics = [];
  for (const rel of walk(workerRoot, root)) {
    if (!rel.endsWith('.rs') || rel.endsWith('/registry.rs')) continue;
    const source = fs.readFileSync(path.join(root, rel), 'utf8');
    if (!/impl\s+(super::)?BackgroundWorker|impl\s+BackgroundWorker/.test(source)) continue;
    if (!/begin_(task|poll|event)\s*\(/.test(source)) {
      diagnostics.push({
        file: rel,
        line: 1,
        message: 'BackgroundWorker must report real lifecycle via begin_task/begin_poll/begin_event',
      });
    }
  }
  return diagnostics;
}

function checkSourceStateConstants(root) {
  const diagnostics = [];
  for (const rel of walk(path.join(root, 'crates'), root)) {
    if (!rel.endsWith('.rs')) continue;
    const source = fs.readFileSync(path.join(root, rel), 'utf8');
    const lines = source.split(/\r?\n/);
    lines.forEach((line, index) => {
      for (const label of SOURCE_STATE_LABELS) {
        const quoted = new RegExp(String.raw`["']${escapeRegExp(label)}["']`);
        if (quoted.test(line) && !SOURCE_LABEL_ALLOW.has(rel)) {
          diagnostics.push({
            file: rel,
            line: index + 1,
            message: `source-state label "${label}" must be referenced through missiond-core constants`,
          });
        }
      }
    });
  }
  return diagnostics;
}

function checkRequiredText(root, rel, expectations) {
  const abs = path.join(root, rel);
  if (!fs.existsSync(abs)) {
    return [{ file: rel, line: 1, message: 'required contract file is missing' }];
  }
  const source = fs.readFileSync(abs, 'utf8');
  return expectations
    .filter(([, pattern]) => !pattern.test(source))
    .map(([name]) => ({ file: rel, line: 1, message: `missing ${name} contract` }));
}

function checkForbiddenText(root, rel, expectations) {
  const abs = path.join(root, rel);
  if (!fs.existsSync(abs)) {
    return [{ file: rel, line: 1, message: 'required contract file is missing' }];
  }
  const source = fs.readFileSync(abs, 'utf8');
  const lines = source.split(/\r?\n/);
  const diagnostics = [];
  for (const [name, pattern] of expectations) {
    lines.forEach((line, index) => {
      if (pattern.test(line)) {
        diagnostics.push({ file: rel, line: index + 1, message: `forbidden ${name}; use AppState context accessors` });
      }
    });
  }
  return diagnostics;
}

function walk(dir, root) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'target') continue;
    const abs = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...walk(abs, root));
    } else if (entry.isFile()) {
      out.push(path.relative(root, abs).split(path.sep).join(path.posix.sep));
    }
  }
  return out;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
