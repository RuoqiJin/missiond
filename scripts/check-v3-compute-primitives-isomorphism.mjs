#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-compute-primitives-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 compute-primitives Lisp/code isomorphism contract:
  - low-level task/job/flow/PTY/process/slot/CC/worker/forge tools are mapped.
  - mission_compute_slot and mission_task_delegate stay outside this surface.
  - daemon dispatch, MCP schemas, and V3 blueprint agree on the public tools.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  dispatcher: 'crates/missiond-daemon/src/handlers/mod.rs',
  computeMod: 'crates/missiond-daemon/src/handlers/compute/mod.rs',
  task: 'crates/missiond-daemon/src/handlers/compute/task.rs',
  job: 'crates/missiond-daemon/src/handlers/compute/job.rs',
  flowRun: 'crates/missiond-daemon/src/handlers/compute/flow_run.rs',
  pty: 'crates/missiond-daemon/src/handlers/compute/pty.rs',
  process: 'crates/missiond-daemon/src/handlers/compute/process.rs',
  slot: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  minimax: 'crates/missiond-daemon/src/handlers/compute/minimax.rs',
  ccTasks: 'crates/missiond-daemon/src/handlers/compute/cc_tasks.rs',
  worker: 'crates/missiond-daemon/src/handlers/compute/worker.rs',
  forge: 'crates/missiond-daemon/src/handlers/compute/forge.rs',
  mcpTask: 'crates/missiond-mcp/src/tools/compute/task.rs',
  mcpJob: 'crates/missiond-mcp/src/tools/compute/job.rs',
  mcpFlowRun: 'crates/missiond-mcp/src/tools/compute/flow_run.rs',
  mcpPty: 'crates/missiond-mcp/src/tools/compute/pty.rs',
  mcpProcess: 'crates/missiond-mcp/src/tools/compute/process.rs',
  mcpSlot: 'crates/missiond-mcp/src/tools/compute/slot.rs',
  mcpMinimax: 'crates/missiond-mcp/src/tools/compute/minimax.rs',
  mcpCcTasks: 'crates/missiond-mcp/src/tools/compute/cc_tasks.rs',
  mcpWorker: 'crates/missiond-mcp/src/tools/compute/worker.rs',
  mcpForge: 'crates/missiond-mcp/src/tools/compute/forge.rs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 compute-primitives Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 compute-primitives Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'compute-primitives',
    '(v2-item compute-primitives',
    ':status code-aligned',
    '(tool-group compute-runtime-tools',
    '(surface compute-primitives',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/handlers/compute/task.rs',
    'crates/missiond-daemon/src/handlers/compute/job.rs',
    'crates/missiond-daemon/src/handlers/compute/flow_run.rs',
    'crates/missiond-daemon/src/handlers/compute/pty.rs',
    'crates/missiond-daemon/src/handlers/compute/process.rs',
    'crates/missiond-daemon/src/handlers/compute/slot.rs',
    'crates/missiond-daemon/src/handlers/compute/minimax.rs',
    'crates/missiond-daemon/src/handlers/compute/cc_tasks.rs',
    'crates/missiond-daemon/src/handlers/compute/worker.rs',
    'crates/missiond-daemon/src/handlers/compute/forge.rs',
    'crates/missiond-mcp/src/tools/compute/task.rs',
    'crates/missiond-mcp/src/tools/compute/job.rs',
    'crates/missiond-mcp/src/tools/compute/flow_run.rs',
    'crates/missiond-mcp/src/tools/compute/pty.rs',
    'crates/missiond-mcp/src/tools/compute/process.rs',
    'crates/missiond-mcp/src/tools/compute/slot.rs',
    'crates/missiond-mcp/src/tools/compute/minimax.rs',
    'crates/missiond-mcp/src/tools/compute/cc_tasks.rs',
    'crates/missiond-mcp/src/tools/compute/worker.rs',
    'crates/missiond-mcp/src/tools/compute/forge.rs',
    'scripts/check-v3-compute-primitives-isomorphism.mjs',
    'task.rs owns mission_task_submit/query/cancel',
    'slot.rs owns mission_slots/mission_inbox/mission_pause/mission_slot_history',
    'compute_slot and task_delegate remain owned by workstation-config',
    'node scripts/check-v3-compute-primitives-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.dispatcher, sources.dispatcher, [
    '"mission_task_submit" | "mission_task_query" | "mission_task_cancel"',
    '"mission_pty_spawn"',
    '"mission_agent" => process::handle',
    '"mission_cc_query" | "mission_cc_swarm"',
    '"mission_worker" | "mission_control"',
    '"mission_job_poll"',
    '"mission_flow_run"',
    '"mission_forge_build" | "mission_forge_lint"',
    '"mission_slots"',
    '"mission_inbox"',
    '"mission_slot_history"',
    'slot::handle(state, name, args).await',
  ]);

  requireAll(diagnostics, files.computeMod, sources.computeMod, [
    'pub(crate) mod cc_tasks;',
    'pub(crate) mod flow_run;',
    'pub(crate) mod forge;',
    'pub(crate) mod job;',
    'pub(crate) mod minimax;',
    'pub(crate) mod process;',
    'pub(crate) mod pty;',
    'pub(crate) mod slot;',
    'pub(crate) mod task;',
    'pub(crate) mod worker;',
  ]);

  requireAll(diagnostics, files.task, sources.task, [
    'mission_task_submit',
    'mission_task_query',
    'mission_task_cancel',
    '"async"',
    '"sync"',
    '"status"',
    '"list"',
    '"ack"',
    '"track"',
    'TaskEvent::Created',
  ]);

  requireAll(diagnostics, files.job, sources.job, [
    '"poll"',
    '"list"',
    '"cancel"',
    'AsyncJobStatus::Running',
  ]);

  requireAll(diagnostics, files.flowRun, sources.flowRun, [
    'mission_flow_run',
    'create_board_task',
    'status: Some("running".to_string())',
    'status: Some("done".to_string())',
    'status: Some("failed".to_string())',
    'resolve_project_root',
  ]);

  requireAll(diagnostics, files.pty, sources.pty, [
    'mission_pty_spawn',
    'mission_pty_send',
    'mission_pty_read',
    'mission_pty_signal',
    'mission_pty_confirm',
    'mission_pty_status',
    'mission_pty_screenshot',
    '"screen"',
    '"history"',
    '"logs"',
    '"kill"',
    '"interrupt"',
    'requeue_running_tasks_for_slot',
    'learned.learn',
  ]);

  requireAll(diagnostics, files.process, sources.process, [
    'mission_agent',
    'mission_spawn',
    'mission_kill',
    'mission_restart',
    'mission_agents',
    'spawn_tracked_slot',
  ]);

  requireAll(diagnostics, files.slot, sources.slot, [
    'mission_slots',
    'mission_inbox',
    'mission_pause',
    'mission_slot_history',
    'global_paused',
    'slot_task_stats',
    'list_slot_tasks',
  ]);

  requireAll(diagnostics, files.minimax, sources.minimax, [
    'mission_minimax_process',
    'mission_sonnet_process',
    'call_interactive',
  ]);

  requireAll(diagnostics, files.ccTasks, sources.ccTasks, [
    'mission_cc_query',
    'mission_cc_swarm',
    'mission_cc_sessions',
    'mission_cc_tasks',
    'mission_cc_overview',
    'mission_cc_in_progress',
    'mission_cc_trigger_swarm',
  ]);

  requireAll(diagnostics, files.worker, sources.worker, [
    'mission_worker',
    'mission_control',
    'mission_workers',
    'mission_worker_control',
    'ControlTree',
  ]);

  requireAll(diagnostics, files.forge, sources.forge, [
    'mission_forge_build',
    'mission_forge_lint',
    'exit_code',
    'project_root',
    'command',
  ]);

  requireAll(diagnostics, files.mcpTask, sources.mcpTask, [
    '"mission_task_submit"',
    '"mission_task_query"',
  ]);
  requireAll(diagnostics, files.mcpJob, sources.mcpJob, ['"mission_job_poll"']);
  requireAll(diagnostics, files.mcpFlowRun, sources.mcpFlowRun, ['"mission_flow_run"']);
  requireAll(diagnostics, files.mcpPty, sources.mcpPty, [
    '"mission_pty_spawn"',
    '"mission_pty_send"',
    '"mission_pty_read"',
    '"mission_pty_signal"',
    '"mission_pty_confirm"',
    '"mission_pty_status"',
    '"mission_pty_screenshot"',
  ]);
  requireAll(diagnostics, files.mcpProcess, sources.mcpProcess, [
    '"mission_agent"',
    '"mission_slots"',
    '"mission_inbox"',
  ]);
  requireAll(diagnostics, files.mcpSlot, sources.mcpSlot, [
    '"mission_slot_history"',
    '"mission_pause"',
  ]);
  requireAll(diagnostics, files.mcpMinimax, sources.mcpMinimax, [
    '"mission_sonnet_process"',
    '"mission_minimax_process"',
  ]);
  requireAll(diagnostics, files.mcpCcTasks, sources.mcpCcTasks, [
    '"mission_cc_query"',
    '"mission_cc_swarm"',
  ]);
  requireAll(diagnostics, files.mcpWorker, sources.mcpWorker, [
    '"mission_worker"',
    '"mission_control"',
  ]);
  requireAll(diagnostics, files.mcpForge, sources.mcpForge, [
    '"mission_forge_build"',
    '"mission_forge_lint"',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required text: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-compute-primitives-'));
  for (const rel of Object.values(DEFAULT_FILES)) {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  }
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.blueprint),
    `
(missiond-blueprint
  (v2-convergence-map
    (v2-item compute-primitives :status code-aligned))
  (public-surface-map
    (tool-group compute-runtime-tools :status code-aligned))
  (implementation-map
    (surface compute-primitives
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/compute/task.rs"
             "crates/missiond-daemon/src/handlers/compute/job.rs"
             "crates/missiond-daemon/src/handlers/compute/flow_run.rs"
             "crates/missiond-daemon/src/handlers/compute/pty.rs"
             "crates/missiond-daemon/src/handlers/compute/process.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "crates/missiond-daemon/src/handlers/compute/minimax.rs"
             "crates/missiond-daemon/src/handlers/compute/cc_tasks.rs"
             "crates/missiond-daemon/src/handlers/compute/worker.rs"
             "crates/missiond-daemon/src/handlers/compute/forge.rs"
             "crates/missiond-mcp/src/tools/compute/task.rs"
             "crates/missiond-mcp/src/tools/compute/job.rs"
             "crates/missiond-mcp/src/tools/compute/flow_run.rs"
             "crates/missiond-mcp/src/tools/compute/pty.rs"
             "crates/missiond-mcp/src/tools/compute/process.rs"
             "crates/missiond-mcp/src/tools/compute/slot.rs"
             "crates/missiond-mcp/src/tools/compute/minimax.rs"
             "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"
             "crates/missiond-mcp/src/tools/compute/worker.rs"
             "crates/missiond-mcp/src/tools/compute/forge.rs"
             "scripts/check-v3-compute-primitives-isomorphism.mjs"]
      :note "task.rs owns mission_task_submit/query/cancel; slot.rs owns mission_slots/mission_inbox/mission_pause/mission_slot_history; compute_slot and task_delegate remain owned by workstation-config."))
  (compression-contract
    :checks ["node scripts/check-v3-compute-primitives-isomorphism.mjs"]))`,
  );
  const fixtureText = 'mission_task_submit mission_task_query mission_task_cancel mission_job_poll mission_flow_run mission_pty_spawn mission_pty_send mission_pty_read mission_pty_signal mission_pty_confirm mission_pty_status mission_pty_screenshot mission_slots mission_slot_history mission_agent mission_inbox mission_sonnet_process mission_minimax_process mission_cc_query mission_cc_swarm mission_worker mission_control mission_pause mission_forge_build mission_forge_lint "mission_task_submit" "mission_task_query" "mission_job_poll" "mission_flow_run" "mission_pty_spawn" "mission_pty_send" "mission_pty_read" "mission_pty_signal" "mission_pty_confirm" "mission_pty_status" "mission_pty_screenshot" "mission_agent" "mission_slots" "mission_inbox" "mission_slot_history" "mission_pause" "mission_sonnet_process" "mission_minimax_process" "mission_cc_query" "mission_cc_swarm" "mission_worker" "mission_control" "mission_forge_build" "mission_forge_lint"';
  for (const rel of Object.values(DEFAULT_FILES)) {
    if (rel === DEFAULT_FILES.blueprint) continue;
    fs.writeFileSync(path.join(root, rel), fixtureText);
  }
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.dispatcher),
    ' "mission_task_submit" | "mission_task_query" | "mission_task_cancel" "mission_pty_spawn" "mission_agent" => process::handle "mission_cc_query" | "mission_cc_swarm" "mission_worker" | "mission_control" "mission_job_poll" "mission_flow_run" "mission_forge_build" | "mission_forge_lint" "mission_slots" "mission_inbox" "mission_slot_history" "mission_pause" => { slot::handle(state, name, args).await }',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.computeMod),
    ' pub(crate) mod cc_tasks; pub(crate) mod flow_run; pub(crate) mod forge; pub(crate) mod job; pub(crate) mod minimax; pub(crate) mod process; pub(crate) mod pty; pub(crate) mod slot; pub(crate) mod task; pub(crate) mod worker;',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.task),
    ' "async" "sync" "status" "list" "ack" "track" TaskEvent::Created',
  );
  fs.appendFileSync(path.join(root, DEFAULT_FILES.job), ' "poll" "list" "cancel" AsyncJobStatus::Running');
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.flowRun),
    ' create_board_task status: Some("running".to_string()) status: Some("done".to_string()) status: Some("failed".to_string()) resolve_project_root',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.pty),
    ' "screen" "history" "logs" "kill" "interrupt" requeue_running_tasks_for_slot learned.learn',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.process),
    ' mission_spawn mission_kill mission_restart mission_agents spawn_tracked_slot',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.slot),
    ' global_paused slot_task_stats list_slot_tasks',
  );
  fs.appendFileSync(path.join(root, DEFAULT_FILES.minimax), ' call_interactive');
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.ccTasks),
    ' mission_cc_sessions mission_cc_tasks mission_cc_overview mission_cc_in_progress mission_cc_trigger_swarm',
  );
  fs.appendFileSync(path.join(root, DEFAULT_FILES.worker), ' mission_workers mission_worker_control ControlTree');
  fs.appendFileSync(path.join(root, DEFAULT_FILES.forge), ' exit_code project_root command');
  return root;
}

main();
