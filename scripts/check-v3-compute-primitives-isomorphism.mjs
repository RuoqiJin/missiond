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
  intentMcpDefs: '.missiond/intent-mcp-defs.lisp',
  dispatcher: 'crates/missiond-daemon/src/handlers/mod.rs',
  computeMod: 'crates/missiond-daemon/src/handlers/compute/mod.rs',
  task: 'crates/missiond-daemon/src/handlers/compute/task.rs',
  job: 'crates/missiond-daemon/src/handlers/compute/job.rs',
  flowRun: 'crates/missiond-daemon/src/handlers/compute/flow_run.rs',
  flowMod: 'crates/missiond-daemon/src/engine/flow/mod.rs',
  flowLoader: 'crates/missiond-daemon/src/engine/flow/loader.rs',
  pty: 'crates/missiond-daemon/src/handlers/compute/pty.rs',
  process: 'crates/missiond-daemon/src/handlers/compute/process.rs',
  slot: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  minimax: 'crates/missiond-daemon/src/handlers/compute/minimax.rs',
  minimaxClient: 'crates/missiond-daemon/src/llm/minimax_client.rs',
  minimaxGateway: 'crates/missiond-daemon/src/llm/minimax_gateway.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  ccTasks: 'crates/missiond-daemon/src/handlers/compute/cc_tasks.rs',
  worker: 'crates/missiond-daemon/src/handlers/compute/worker.rs',
  forge: 'crates/missiond-daemon/src/handlers/compute/forge.rs',
  v3Runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
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
    ':status runtime-projected',
    '(tool-group compute-runtime-tools',
    '(surface compute-primitives',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/handlers/compute/task.rs',
    'crates/missiond-daemon/src/handlers/compute/job.rs',
    'crates/missiond-daemon/src/handlers/compute/flow_run.rs',
    'crates/missiond-daemon/src/engine/flow/mod.rs',
    'crates/missiond-daemon/src/engine/flow/loader.rs',
    'crates/missiond-daemon/src/handlers/compute/pty.rs',
    'crates/missiond-daemon/src/handlers/compute/process.rs',
    'crates/missiond-daemon/src/handlers/compute/slot.rs',
    'crates/missiond-daemon/src/handlers/compute/minimax.rs',
    'crates/missiond-daemon/src/llm/minimax_client.rs',
    'crates/missiond-daemon/src/llm/minimax_gateway.rs',
    'crates/missiond-daemon/src/handlers/compute/cc_tasks.rs',
    'crates/missiond-daemon/src/handlers/compute/worker.rs',
    'crates/missiond-daemon/src/handlers/compute/forge.rs',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
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
    'flow-runtime-policy',
    'compute-runtime-policy',
    'minimax-runtime-policy',
    'timeout-policy tracked-pty-spawn',
    'mission_flow_run MUST project missing FlowDefinition node defaults from flow-runtime-policy',
    'Explicit Flow YAML node fields MUST win over flow-runtime-policy defaults',
    'mission_agent spawn/restart and mission_task_submit auto-spawn MUST project tracked PTY spawn wait_for_idle timeout from compute-runtime-policy timeout-policy tracked-pty-spawn',
    'MiniMaxClient HTTP timeout and default max_tokens MUST project from minimax-runtime-policy',
    'MinimaxGateway quota throttle sleep MUST project from minimax-runtime-policy',
    'mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm',
    'mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking',
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

  requireAll(diagnostics, files.intentMcpDefs, sources.intentMcpDefs, [
    '(tool mission_cc_swarm',
    '(timeoutMs number :default 600000 :description "PTY 等待超时；默认/上下限由 V3 workstation-config timeout-policy claudecode-swarm 投影")',
    '(tool mission_pty_send',
    '(timeoutMs number :default 300000 :description "[waitForResponse=true] 默认/上下限由 V3 workstation-config timeout-policy pty-send-blocking 投影")',
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
    'ComputePrimitivesRuntimeConfig::load_for_project_root',
    'pty_spawn_timeout_secs',
    'timeout_secs: Some(spawn_timeout_secs)',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);
  forbidAll(diagnostics, files.task, sources.task, ['timeout_secs: Some(30)']);

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
    'load_flow_from_path_with_project',
  ]);

  requireAll(diagnostics, files.flowMod, sources.flowMod, [
    'DEFAULT_FLOW_LLM_MAX_TOKENS',
    'DEFAULT_FLOW_SLOT_MODEL',
    'DEFAULT_FLOW_SLOT_TIMEOUT_SECS',
    'DEFAULT_FLOW_PARALLELISM',
    'DEFAULT_FLOW_PARALLEL_TIMEOUT_SECS',
  ]);

  requireAll(diagnostics, files.flowLoader, sources.flowLoader, [
    'FlowRuntimeConfig::load_for_project_root',
    'apply_flow_runtime_defaults',
    'yaml_key_missing',
    'load_flow_from_path_with_project',
    'config.slot_task_default_model.clone()',
    'config.slot_task_default_timeout_secs',
    'config.parallel_slot_default_parallelism',
    'config.parallel_slot_default_timeout_secs',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);

  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'pub(crate) struct FlowRuntimeConfig',
    'pub(crate) struct ComputePrimitivesRuntimeConfig',
    'DEFAULT_FLOW_LLM_MAX_TOKENS',
    'DEFAULT_FLOW_SLOT_MODEL',
    'DEFAULT_FLOW_SLOT_TIMEOUT_SECS',
    'DEFAULT_FLOW_PARALLELISM',
    'DEFAULT_FLOW_PARALLEL_TIMEOUT_SECS',
    'DEFAULT_COMPUTE_PTY_SPAWN_TIMEOUT_SECS',
    'MIN_COMPUTE_PTY_SPAWN_TIMEOUT_SECS',
    'MAX_COMPUTE_PTY_SPAWN_TIMEOUT_SECS',
    'MinimaxRuntimeConfig',
    'DEFAULT_MINIMAX_DIRECT_HTTP_TIMEOUT_SECS',
    'DEFAULT_MINIMAX_QUOTA_THROTTLE_SECS',
    'DEFAULT_MINIMAX_MAX_TOKENS',
    'pub(crate) fn parse_flow_runtime_policy',
    'pub(crate) fn parse_compute_runtime_policy',
    'pub(crate) fn parse_minimax_runtime_policy',
    'flow-runtime-policy',
    'compute-runtime-policy',
    'minimax-runtime-policy',
    'tracked-pty-spawn',
    'pty_spawn_timeout_secs',
    'direct_http_timeout',
    'quota_throttle_sleep',
    ':default-max-tokens',
    ':slot-task-default-model',
    ':parallel-slot-default-timeout-secs',
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
    'WorkstationRuntimeConfig::load_for_project_root',
    'slot_project_root',
    'clamp_pty_send_timeout_ms',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'ToolResult::structured_error',
  ]);

  requireAll(diagnostics, files.process, sources.process, [
    'mission_agent',
    'mission_spawn',
    'mission_kill',
    'mission_restart',
    'mission_agents',
    'spawn_tracked_slot',
    'ComputePrimitivesRuntimeConfig::load_for_project_root',
    'pty_spawn_timeout_secs',
    'timeout_secs: Some(spawn_timeout_secs)',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);
  forbidAll(diagnostics, files.process, sources.process, ['timeout_secs: Some(30)']);

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

  requireAll(diagnostics, files.minimaxClient, sources.minimaxClient, [
    'MinimaxRuntimeConfig',
    'new_with_runtime_config',
    'config.direct_http_timeout()',
    'config.default_max_tokens',
    'self.default_max_tokens',
  ]);
  forbidAll(diagnostics, files.minimaxClient, sources.minimaxClient, [
    'DEFAULT_TIMEOUT_SECS',
    'DEFAULT_MAX_TOKENS',
    'Duration::from_secs(30)',
  ]);

  requireAll(diagnostics, files.minimaxGateway, sources.minimaxGateway, [
    'MinimaxRuntimeConfig::load_for_current_dir',
    'MiniMaxClient::new_with_runtime_config',
    'quota_throttle_sleep',
    'runtime_config.quota_throttle_sleep()',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);
  forbidAll(diagnostics, files.minimaxGateway, sources.minimaxGateway, [
    'Duration::from_secs(60)',
    'throttling 60s',
  ]);

  requireAll(diagnostics, files.main, sources.main, [
    'minimax_gateway::create_minimax_gateway()?',
  ]);

  requireAll(diagnostics, files.ccTasks, sources.ccTasks, [
    'mission_cc_query',
    'mission_cc_swarm',
    'mission_cc_sessions',
    'mission_cc_tasks',
    'mission_cc_overview',
    'mission_cc_in_progress',
    'mission_cc_trigger_swarm',
    'WorkstationRuntimeConfig::load_for_project_root',
    'slot_project_root',
    'clamp_cc_swarm_timeout_ms',
    'V3_BLUEPRINT_CONFIG_ERROR',
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
    'timeout-policy pty-send-blocking',
    '"default": 300000',
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
    'timeout-policy claudecode-swarm',
    '"default": 600000',
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

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden text is still present: ${needle}` });
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
    (v2-item compute-primitives :status runtime-projected))
  (public-surface-map
    (tool-group compute-runtime-tools :status code-aligned))
  (flow-runtime-policy
    :llm-call-default-max-tokens 65536
    :slot-task-default-model "opus"
    :slot-task-default-timeout-secs 3600
    :parallel-slot-default-parallelism 3
    :parallel-slot-default-timeout-secs 1800
    :invariants ["mission_flow_run MUST project missing FlowDefinition node defaults from flow-runtime-policy"
                 "Explicit Flow YAML node fields MUST win over flow-runtime-policy defaults"
                 "mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm"
                 "mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking"])
  (compute-runtime-policy
    (timeout-policy tracked-pty-spawn
      :default_secs 30
      :min_secs 1
      :max_secs 600)
    :invariants ["mission_agent spawn/restart and mission_task_submit auto-spawn MUST project tracked PTY spawn wait_for_idle timeout from compute-runtime-policy timeout-policy tracked-pty-spawn"])
  (minimax-runtime-policy
    :direct-http-timeout-secs 30
    :quota-throttle-secs 60
    :default-max-tokens 500
    :invariants ["MiniMaxClient HTTP timeout and default max_tokens MUST project from minimax-runtime-policy"
                 "MinimaxGateway quota throttle sleep MUST project from minimax-runtime-policy"])
  (implementation-map
    (surface compute-primitives
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/compute/task.rs"
             "crates/missiond-daemon/src/handlers/compute/job.rs"
             "crates/missiond-daemon/src/handlers/compute/flow_run.rs"
             "crates/missiond-daemon/src/engine/flow/mod.rs"
             "crates/missiond-daemon/src/engine/flow/loader.rs"
             "crates/missiond-daemon/src/handlers/compute/pty.rs"
             "crates/missiond-daemon/src/handlers/compute/process.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "crates/missiond-daemon/src/handlers/compute/minimax.rs"
             "crates/missiond-daemon/src/llm/minimax_client.rs"
             "crates/missiond-daemon/src/llm/minimax_gateway.rs"
             "crates/missiond-daemon/src/handlers/compute/cc_tasks.rs"
             "crates/missiond-daemon/src/handlers/compute/worker.rs"
             "crates/missiond-daemon/src/handlers/compute/forge.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
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
      :note "task.rs owns mission_task_submit/query/cancel; flow-runtime-policy projects mission_flow_run defaults; engine/flow/loader.rs loads flow-runtime-policy and preserves explicit fields; llm/minimax_client.rs and llm/minimax_gateway.rs project direct MiniMax timeout/max_tokens/quota throttle from minimax-runtime-policy for background lanes; slot.rs owns mission_slots/mission_inbox/mission_pause/mission_slot_history; compute_slot and task_delegate remain owned by workstation-config."))
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
    path.join(root, DEFAULT_FILES.intentMcpDefs),
    ' (tool mission_cc_swarm (timeoutMs number :default 600000 :description "PTY 等待超时；默认/上下限由 V3 workstation-config timeout-policy claudecode-swarm 投影")) (tool mission_pty_send (timeoutMs number :default 300000 :description "[waitForResponse=true] 默认/上下限由 V3 workstation-config timeout-policy pty-send-blocking 投影"))',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.computeMod),
    ' pub(crate) mod cc_tasks; pub(crate) mod flow_run; pub(crate) mod forge; pub(crate) mod job; pub(crate) mod minimax; pub(crate) mod process; pub(crate) mod pty; pub(crate) mod slot; pub(crate) mod task; pub(crate) mod worker;',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.task),
    ' "async" "sync" "status" "list" "ack" "track" TaskEvent::Created ComputePrimitivesRuntimeConfig::load_for_project_root pty_spawn_timeout_secs timeout_secs: Some(spawn_timeout_secs) V3_BLUEPRINT_CONFIG_ERROR',
  );
  fs.appendFileSync(path.join(root, DEFAULT_FILES.job), ' "poll" "list" "cancel" AsyncJobStatus::Running');
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.flowRun),
    ' create_board_task status: Some("running".to_string()) status: Some("done".to_string()) status: Some("failed".to_string()) resolve_project_root load_flow_from_path_with_project',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.flowMod),
    ' DEFAULT_FLOW_LLM_MAX_TOKENS DEFAULT_FLOW_SLOT_MODEL DEFAULT_FLOW_SLOT_TIMEOUT_SECS DEFAULT_FLOW_PARALLELISM DEFAULT_FLOW_PARALLEL_TIMEOUT_SECS',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.flowLoader),
    ' FlowRuntimeConfig::load_for_project_root apply_flow_runtime_defaults yaml_key_missing load_flow_from_path_with_project config.slot_task_default_model.clone() config.slot_task_default_timeout_secs config.parallel_slot_default_parallelism config.parallel_slot_default_timeout_secs V3_BLUEPRINT_CONFIG_ERROR',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.v3Runtime),
    ' pub(crate) struct FlowRuntimeConfig pub(crate) struct ComputePrimitivesRuntimeConfig MinimaxRuntimeConfig DEFAULT_FLOW_LLM_MAX_TOKENS DEFAULT_FLOW_SLOT_MODEL DEFAULT_FLOW_SLOT_TIMEOUT_SECS DEFAULT_FLOW_PARALLELISM DEFAULT_FLOW_PARALLEL_TIMEOUT_SECS DEFAULT_COMPUTE_PTY_SPAWN_TIMEOUT_SECS MIN_COMPUTE_PTY_SPAWN_TIMEOUT_SECS MAX_COMPUTE_PTY_SPAWN_TIMEOUT_SECS DEFAULT_MINIMAX_DIRECT_HTTP_TIMEOUT_SECS DEFAULT_MINIMAX_QUOTA_THROTTLE_SECS DEFAULT_MINIMAX_MAX_TOKENS pub(crate) fn parse_flow_runtime_policy pub(crate) fn parse_compute_runtime_policy pub(crate) fn parse_minimax_runtime_policy flow-runtime-policy compute-runtime-policy minimax-runtime-policy tracked-pty-spawn pty_spawn_timeout_secs direct_http_timeout quota_throttle_sleep :default-max-tokens :slot-task-default-model :parallel-slot-default-timeout-secs',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.pty),
    ' "screen" "history" "logs" "kill" "interrupt" requeue_running_tasks_for_slot learned.learn WorkstationRuntimeConfig::load_for_project_root slot_project_root clamp_pty_send_timeout_ms V3_BLUEPRINT_CONFIG_ERROR ToolResult::structured_error',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.process),
    ' mission_spawn mission_kill mission_restart mission_agents spawn_tracked_slot ComputePrimitivesRuntimeConfig::load_for_project_root pty_spawn_timeout_secs timeout_secs: Some(spawn_timeout_secs) V3_BLUEPRINT_CONFIG_ERROR',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.slot),
    ' global_paused slot_task_stats list_slot_tasks',
  );
  fs.appendFileSync(path.join(root, DEFAULT_FILES.minimax), ' call_interactive');
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.minimaxClient),
    ' MinimaxRuntimeConfig new_with_runtime_config config.direct_http_timeout() config.default_max_tokens self.default_max_tokens',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.minimaxGateway),
    ' MinimaxRuntimeConfig::load_for_current_dir MiniMaxClient::new_with_runtime_config quota_throttle_sleep runtime_config.quota_throttle_sleep() V3_BLUEPRINT_CONFIG_ERROR tokio::time::sleep(self.quota_throttle_sleep).await',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.main),
    ' minimax_gateway::create_minimax_gateway()?',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.ccTasks),
    ' mission_cc_sessions mission_cc_tasks mission_cc_overview mission_cc_in_progress mission_cc_trigger_swarm WorkstationRuntimeConfig::load_for_project_root slot_project_root clamp_cc_swarm_timeout_ms V3_BLUEPRINT_CONFIG_ERROR',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.mcpCcTasks),
    ' timeout-policy claudecode-swarm "default": 600000',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.mcpPty),
    ' timeout-policy pty-send-blocking "default": 300000',
  );
  fs.appendFileSync(path.join(root, DEFAULT_FILES.worker), ' mission_workers mission_worker_control ControlTree');
  fs.appendFileSync(path.join(root, DEFAULT_FILES.forge), ' exit_code project_root command');
  return root;
}

main();
