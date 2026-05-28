#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-grounded-dispatch-isomorphism.mjs [--json] [--repo <path>]

Checks the V3 grounded-dispatch contract:
  - SSOT declares grounded-dispatch-policy, context-gather-artifact,
    task-delegate-grounding-gate, and autopilot-grounding-gate.
  - mission_context_gather can persist a durable context-gather artifact and
    return grounding_context_id/context_pack_path.
  - mission_task_delegate and mission_swarm_run enforce grounding before broad
    dispatch and fail fast when gather cannot produce a grounded artifact.
  - Autopilot refuses ungrounded broad BoardTasks before PTY dispatch.
  - Jarvis worker dispatch uses the runtime/project root for read_scope.
    Provider/Autopilot completion writes canonical task-result-artifacts before
    streaming result_artifact events; Jarvis follow-up must never promote Board
    summary notes into canonical artifacts.
  - Jarvis plan-confirmed dispatch emits a worker_dispatched/pending claim event
    before returning result_pending, and provider prompts no longer delegate the
    primary BoardTask done transition to workers.
`;

let json = false;
let repoRoot = process.cwd();
const args = process.argv.slice(2);
for (let i = 0; i < args.length; i += 1) {
  const arg = args[i];
  if (arg === '--json') {
    json = true;
  } else if (arg === '--repo') {
    repoRoot = path.resolve(args[++i] ?? '');
  } else if (arg === '--help' || arg === '-h') {
    console.log(usage);
    process.exit(0);
  } else {
    console.error(`unknown arg: ${arg}`);
    console.error(usage);
    process.exit(2);
  }
}

const checks = [
  [
    'ssot grounded-dispatch policy',
    '.missiond/v3/shards/request-runtime.lisp',
    [
      '(grounded-dispatch-policy',
      ':schema "missiond.grounded-dispatch-policy.v1"',
      '(function context-gather-artifact',
      '(function task-delegate-grounding-gate',
      '(function autopilot-grounding-gate',
      '(function jarvis-result-followup',
      'grounding_context_id',
      'missiond_follow_task_id',
      'result_pending',
      'short-poll budget',
      'MISSIOND_JARVIS_VISIBLE_HEARTBEAT_SECS',
      'visible worker_status heartbeat',
      'before default-slot readiness checks',
      'busy worker slot during follow is a progress state',
      'broad investigation/design/planning objectives as review/read-only',
      'Task/Explore/TaskCreate/TaskUpdate/TaskList/TaskOutput',
      'Web/iOS clients and smoke tests MUST automatically follow',
      'missiond_confirm',
      'missiond_objective',
      'mission_context_gather(persist=true)',
      'GROUNDING_REQUIRED',
      'node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json',
    ],
  ],
  [
    'intent workflow runtime gate',
    '.missiond/workflows/intent-intake-grounding.lisp',
    [
      'mission_context_gather',
      'persisted grounding_context_id',
      'exact_shard_ready',
      'accepted_shard_id, context_pack_path, and write_scope',
    ],
  ],
  [
    'work-order lifecycle grounding gate',
    '.missiond/workflows/work-order-lifecycle.lisp',
    [
      '(gate grounding-required-before-worker',
      'mission_context_gather(persist=true)',
      'grounding_context_id',
      'runtime block',
    ],
  ],
  [
    'context gather persistent artifact',
    'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
    [
      'persist: Option<bool>',
      'task_id: Option<String>',
      'source_id: Option<String>',
      '"missiond.context-gather-artifact.v1"',
      'put_json_artifact',
      '"context-gather"',
      '"grounding_context_id"',
      '"context_pack_path"',
      '"context_pack_file"',
      'CONTEXT_GATHER_RUNTIME_REL',
      'CONTEXT_GATHER_WORKER_VISIBLE_REL',
      'materialize_context_pack_file',
      'canonical_context_pack_file',
      'context_gather_worker_visible_dir',
      '"shared-artifact://',
      '"sources_used"',
    ],
  ],
  [
    'shared memory JSON artifact helper',
    'crates/missiond-daemon/src/engine/shared_memory.rs',
    [
      'pub(crate) async fn put_json_artifact',
      'put_artifact_bytes',
      'serde_json::to_vec',
      '"application/json"',
    ],
  ],
  [
    'task delegate grounding gate',
    'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
    [
      'dispatch_grounding_required',
      'gather_dispatch_grounding',
      'apply_grounding_to_metadata',
      'context_gather::handle',
      '"mission_context_gather"',
      '"persist": true',
      '"GROUNDING_REQUIRED"',
      'grounding_context_id',
      'grounding_sources',
      'context_pack_path',
    ],
  ],
  [
    'swarm grounding contract',
    'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
    [
      '"SWARM_GROUNDING_CONTEXT_REQUIRED"',
      'grounding_context_id',
      'grounding_artifact',
      ':grounding_context_id',
      '- grounding_context_id:',
      'accepted shards must reference an existing grounding_context_id',
    ],
  ],
  [
    'autopilot grounding gate',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    [
      'autopilot_grounding_gate_reason',
      'block_task_for_grounding_required',
      'board_task_exact_shard_ready',
      'grounding_context_id',
      'Autopilot refused broad dispatch without grounding_context_id',
      'GROUNDING_REQUIRED',
    ],
  ],
  [
    'jarvis dispatch verb classification',
    'crates/missiond-core/src/ws/server.rs',
    [
      'fn classify_jarvis_dispatch_verb',
      'READ_ONLY_MARKERS',
      'INVESTIGATION_FIRST_MARKERS',
      'CODE_NOW_MARKERS',
      'no file changes',
      '不要修改',
      '不要提交',
      'jarvis-grounded-implementation',
      'codex-grounded-implementation',
      '"scoped"',
      '"read-only"',
      'claude-code-default',
      'codex-code-worker',
      'engine_hint=codex',
      '工位实现任务',
      'write_scope 范围内修改文件',
      'Task/Explore/TaskCreate/TaskUpdate/TaskList/TaskOutput',
    ],
  ],
  [
    'provider final terminal turn gate',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    [
      'assistant_message_can_be_terminal_final',
      'metadata_stop_reason',
      'stop_reason',
      'tool_use',
      'provider_completion_rejects_tool_use_stop_reason_even_when_text_looks_final',
    ],
  ],
  [
    'claude stop reason preservation',
    'crates/missiond-daemon/src/infra/message_handler.rs',
    [
      'stop_reason',
      'provider',
      'claude_code',
    ],
  ],
  [
    'jarvis runtime read-scope root',
    'crates/missiond-core/src/ws/server.rs',
    [
      'fn jarvis_runtime_read_scope_root',
      'fn jarvis_dispatch_read_scope',
      'MISSIOND_PROJECT_ROOT',
      'MISSIOND_REPO_ROOT',
      'MISSIOND_WORKSPACE_ROOT',
      '"read_scope": read_scope',
      'context_pack_file',
    ],
  ],
  [
    'jarvis task-result-artifact stream projection',
    'crates/missiond-core/src/ws/server.rs',
    [
      'jarvis_artifact_writer: &JarvisArtifactSlot',
      '"TASK_RESULT_ARTIFACT_REQUIRED"',
      'MISSIOND_JARVIS_PUBLIC_STREAM_BUDGET_SECS',
      'const DEFAULT_BUDGET_SECS: u64 = 6',
      'MISSIOND_JARVIS_VISIBLE_HEARTBEAT_SECS',
      '"heartbeat": true',
      'jarvis_confirm_bool',
      'jarvis_confirm_string',
      'openai_request_follow_task_id',
      'interaction_metadata_string',
      'missiond_objective',
      'confirmation_objective',
      '"context_pack_file"',
      'context_pack_file',
      'context unavailable',
      'missiond_follow_task_id',
      '"result_pending"',
      '"dispatch_accepted"',
      '"worker_dispatched"',
      '"pending_autopilot_claim"',
      '"terminal_task_result": false',
      '"result_followup"',
      'handle_chat_completions_interaction_adapter',
      'ensure_jarvis_slot_ready_for_chat',
      'jarvis_follow_request_is_detected_before_slot_readiness',
      '"jarvis-follow-board-projection"',
      '"artifact_hash": latest_artifact_hash.clone()',
      'get_board_task_projection_artifact_hash',
      'do not parse Board notes for artifact hashes',
      'Self::put_jarvis_artifact',
      'jarvis_follow_does_not_synthesize_task_result_from_board_summary',
      'mission_shared_memory(action=\\"artifact_get\\"',
      'mission_context_slice',
    ],
  ],
  [
    'jarvis intent/plan dispatch follow smoke',
    'scripts/smoke-jarvis-intent-plan-dispatch.mjs',
    [
      'JARVIS_DISPATCH_FOLLOW',
      'JARVIS_FOLLOW_TASK_ID',
      'findFollowTaskId',
      'followTaskUntilTerminal',
      'request timeout',
      'FOLLOW_TERMINAL_RESULT_MISSING',
      'FOLLOW_FINAL_WITHOUT_RESULT_ARTIFACT',
    ],
  ],
  [
    'stale BoardTask final audit',
    'scripts/audit-stale-boardtask-finals.mjs',
    [
      'missiond.stale-boardtask-final-audit.v1',
      'CONVERSATION_ENDED_BEFORE_CLAIM',
      'TERMINAL_TASK_WITHOUT_ARTIFACT_HASH',
      'mission_board_query',
      'mission_conversation_query',
      'task-result-artifact',
    ],
  ],
  [
    'autopilot task-result candidate observation',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    [
      'observe_autopilot_task_result_candidate',
      '"schema": "missiond.task-result-candidate.v1"',
      'let raw_evidence = durable_completion',
      '"raw_evidence": raw_evidence',
      '"summary": final_summary',
      'completed_task_result_artifact_hash_for_task',
      'settle_autopilot_done_with_artifact',
      'extract_worker_final_summary_prefers_findings_contract_block',
      'contains_worker_prompt_contract_echo',
      'echoed-worker-prompt-contract',
      'pty_summary_rejects_echoed_worker_prompt_contract',
    ],
  ],
  [
    'durable final selection strips thinking blocks from candidates',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    [
      'fn visible_text_from_message',
      'visible_text_from_message(msg)',
      'fn settle_worker_conversation',
      'settle_worker_conversation(state',
    ],
  ],
  [
    'autopilot provider prompt close ownership',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    [
      'artifact-owned completion guidance',
      'task-result-artifact',
      '不要调用 `mission_board_update(status=\\"done\\")`',
      'Autopilot/orchestrator 会在 durable final 和 task-result-artifact 验证后负责关闭此 BoardTask。',
    ],
  ],
  [
    'daemon task-result-artifact writer',
    'crates/missiond-daemon/src/main.rs',
    [
      'if req.kind == "task-result-artifact"',
      '"task-result-artifact requires task_id"',
      'TaskCompletionEvidenceWriter::new',
      '.write_bounded(',
      'artifact_id: format!("task-result-artifact:{hash}")',
      'path: format!("shared-artifact://{hash}")',
    ],
  ],
  [
    'aggregate registration',
    'scripts/check-v3-code-isomorphism-complete.mjs',
    ['scripts/check-v3-grounded-dispatch-isomorphism.mjs'],
  ],
  [
    'autopilot empty final diagnostic',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    [
      'provider-empty-final',
      'provider returned an empty final response after slot',
      'recording observation only',
      'no terminal state written',
    ],
  ],
];

const forbiddenChecks = [
  [
    'jarvis dispatch must not hardcode local developer root',
    'crates/missiond-core/src/ws/server.rs',
    ['"read_scope": ["/Users/jinchen/Projects/missiond"]'],
  ],
  [
    'jarvis follow must not synthesize canonical artifacts from Board notes',
    'crates/missiond-core/src/ws/server.rs',
    ['"jarvis-board-summary-projection"', '"jarvis-terminal-revalidate"', 'kind: "task-result-artifact".to_string()'],
  ],
];

const diagnostics = [];
for (const [label, rel, needles] of checks) {
  const file = path.join(repoRoot, rel);
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (error) {
    diagnostics.push({
      label,
      file: rel,
      message: `missing file: ${error.message}`,
    });
    continue;
  }
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({
        label,
        file: rel,
        needle,
        message: `missing grounded-dispatch anchor: ${needle}`,
      });
    }
  }
}

for (const [label, rel, forbiddenNeedles] of forbiddenChecks) {
  const file = path.join(repoRoot, rel);
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (error) {
    diagnostics.push({
      label,
      file: rel,
      message: `missing file: ${error.message}`,
    });
    continue;
  }
  for (const needle of forbiddenNeedles) {
    if (text.includes(needle)) {
      diagnostics.push({
        label,
        file: rel,
        needle,
        message: `forbidden grounded-dispatch anchor present: ${needle}`,
      });
    }
  }
}

const result = {
  ok: diagnostics.length === 0,
  checked: checks.length,
  diagnostics,
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else if (result.ok) {
  console.log('v3 grounded-dispatch Lisp/code isomorphism check OK');
} else {
  for (const d of diagnostics) {
    console.error(`${d.file}: ${d.message}`);
  }
  console.error(
    `v3 grounded-dispatch Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
  );
}

process.exit(result.ok ? 0 : 1);
