#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  frontendBlueprint: '.missiond/frontend/board-blueprint.lisp',
  runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  masterControl: 'crates/missiond-daemon/src/engine/master_control.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  boardEvents: 'crates/missiond-daemon/src/handlers/knowledge/board/events.rs',
  boardNote: 'crates/missiond-daemon/src/handlers/knowledge/board/note.rs',
  slotHandler: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  mcpProcessTools: 'crates/missiond-mcp/src/tools/compute/process.rs',
  mcpToolsMod: 'crates/missiond-mcp/src/tools/mod.rs',
  slotTypes: 'crates/missiond-core/src/types/slot.rs',
  ptyManager: 'crates/missiond-pty/src/manager.rs',
  ptySession: 'crates/missiond-pty/src/session.rs',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
};

function main() {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = fs.readFileSync(path.join(process.cwd(), rel), 'utf8');
    } catch (err) {
      diagnostics.push(`${rel}: cannot read: ${err.message}`);
    }
  }
  if (diagnostics.length === 0) check(sources, diagnostics);
  if (diagnostics.length > 0) {
    diagnostics.forEach((d) => console.error(d));
    console.error(`v3 master-control isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
    process.exit(1);
  }
  console.log('v3 master-control isomorphism check OK');
}

function check(s, diagnostics) {
  requireAll(diagnostics, FILES.blueprint, s.blueprint, [
    '(resident-master-control',
    ':worker codex-master-control',
    ':slot-id "slot-codex-master-control"',
    ':model-profile codex-master-gpt-5-5-xhigh',
    ':model "gpt-5.5"',
    ':reasoning-effort xhigh',
    ':checkpoint',
    ':event-subscriptions',
    ':evidence-authority',
    ':settle-policy',
    ':surfaces [master-checkpoint master-event-subscriber master-decision-loop master-delegation master-recovery night-scheduler]',
    '(master-checkpoint',
    '(master-event-subscriber',
    'live-only v2 subscription names, StartFrom::Latest, and PerEvent cursor flush',
    'ignore slot-codex-master-control self SlotEvents',
    'ignore seq=0 volatile events and ordinary SlotEvent.became_idle noise',
    'filter low-value Board completion acknowledgements before the model',
    'status_changed->done/completed/closed',
    'same-process Board tool handlers also call notify_board_event_direct',
    'Board notes authored by codex-master-control MUST NOT direct-notify the master again',
    '(master-decision-loop',
    'event_cursor + event_summary',
    'require MissionD MCP first (mission_intent, mission_board_query, mission_conversation_query, mission_kb_query)',
    'call mission_task_delegate directly for delegation requests',
    'mission_compute_slot',
    'use mission_board_note_add for progress/summary notes',
    'on daemon-startup, ensure slot-codex-master-control is spawned when Exited/Error but do not consume a control turn',
    'ensure slot-codex-master-control is spawned when Exited/Error',
    'wait briefly for Idle/SlashMenu',
    'verify the visible Codex footer still matches gpt-5.5 xhigh',
    'send control turns to slot-codex-master-control only on event-wakeup',
    '(master-delegation',
    '(master-recovery',
    '(night-scheduler',
    ':mcp-readiness',
    ':probe "codex mcp list"',
    ':required-tool-approvals',
    'mission_master_status.mcpApprovalReady',
    'detect code-first diffs and create a deduped backfill BoardTask',
    'never auto-hide, skip, delete, or mutate historical Board cleanup candidates',
    'mission_master_status',
    'resolve checkpoint root from the V3-projected master slot project_root/cwd',
    'without calling notify or incrementing queued control events',
    'last-control-prompt is nil for heartbeat/startup ticks',
    'node scripts/check-v3-master-control-isomorphism.mjs',
    '(surface resident-master-control',
  ]);
  requireAll(diagnostics, FILES.blueprint, s.blueprint, [
    '(worker codex-master-control',
    ':engine codex',
    ':role orchestrator',
    ':search true',
    ':sandbox read-only',
    ':approval-policy never',
    ':accepts-boardtask false',
    ':write-allowed false',
  ]);
  requireAll(diagnostics, FILES.runtime, s.runtime, [
    'DEFAULT_CODEX_MASTER_PROFILE',
    '"codex-master-control"',
    '"slot-codex-master-control"',
    'reasoning_effort: Some("xhigh".to_string())',
    'search_enabled: true',
    'workstation-pool must include a non-shard Codex master-control worker',
  ]);
  requireAll(diagnostics, FILES.main, s.main, [
    'reasoning_effort: worker.reasoning_effort.clone()',
    'search_enabled: Some(worker.search_enabled).filter',
    'approval_policy: worker.approval_policy.clone()',
    'engine::master_control::write_startup_checkpoint_for_slot',
    'engine::master_control::start_master_control_service',
  ]);
  requireAll(diagnostics, FILES.masterControl, s.masterControl, [
    'pub(crate) struct MasterControlService',
    'pub(crate) enum WorkerCompletionEvidence',
    'fn record_checkpoint_context',
    'fn spawn_master_event_subscriber',
    'fn should_wake_for_board_event',
    'fn slot_event_slot_id',
    'fn should_wake_for_slot_event',
    'pub(crate) fn notify_board_event_direct',
    'seq > 0',
    'StatusChanged',
    'status != "done"',
    'SlotEvent::BecameIdle',
    'should_wake_for_slot_event(ack.event(), seq)',
    'slot_event_slot_id(event) != MASTER_SLOT_ID',
    'fn spawn_master_decision_loop',
    'BoardEvent',
    'SlotEvent',
    'QuestionEvent',
    'MASTER_BOARD_SUBSCRIPTION',
    'MASTER_SLOT_SUBSCRIPTION',
    'MASTER_QUESTION_SUBSCRIPTION',
    'master_live_subscription_opts',
    'StartFrom::Latest',
    'CursorFlush::PerEvent',
    'master-control-checkpoint.lisp',
    'build_master_tick_prompt',
    'prompt_preview: None',
    'should_dispatch_control_turn(reason, &snapshot, mcp_ready).then',
    'event_summary',
    'mission_intent(project=\\"missiond\\", action=\\"summary\\")',
	    'For BoardTaskCreated/Updated, query that BoardTask by id before deciding',
	    'For implementation work, use the two-stage context-pack workflow',
	    'context_pack_path, write_scope, must_not_touch, acceptance, model_profile, timeout_secs, task_class, pool_hint, and engine_hint',
	    'call mission_task_delegate directly',
    'mission_board_note_add',
    'Do not run broad shell scans',
    'dispatch_control_turn',
    'ensure_master_slot_expected_model',
    'codex_master_model_mismatch',
    'master-control Codex slot model mismatch',
    'ensure_master_slot_running',
    'spawn_tracked_slot',
    'auto_restart: true',
    'wait_for_master_slot_ready',
    'SessionState::Idle | SessionState::SlashMenu',
    'master slot did not become idle',
    'send_fire_and_forget(MASTER_SLOT_ID, prompt)',
    'should_dispatch_control_turn',
    'reason != "event-wakeup"',
    'MISSIOND_MASTER_CONTROL_TURNS',
    'probe_codex_mcp_ready',
    'probe_codex_mcp_control_ready',
    'probe_codex_mcp_approval_ready',
    'codex_mcp_approval_ready_from_config',
    'MASTER_MCP_APPROVED_TOOLS',
    'approval_mode = \\"approve\\"',
    'Command::new("codex").args(["mcp", "list"])',
    'codex_mcp_ready_from_output',
    'ensure_code_drift_backfill_task',
    'detect_code_first_drift',
    'CreateBoardTaskInput',
    'lisp-code-drift:',
    'find(|slot| slot.config.id == MASTER_SLOT_ID)',
    'slot.config.project_root.or(slot.config.cwd)',
    'mcp_ready',
    'event_cursor',
    't3-diagnostic-only',
  ]);
  requireAll(diagnostics, FILES.boardEvents, s.boardEvents, [
    'notify_board_event_direct(&ev)',
    'bus.publish_board(ev)',
  ]);
  requireAll(diagnostics, FILES.boardNote, s.boardNote, [
    'is_master_control_note',
    'crate::engine::master_control::MASTER_WORKER_ID',
    'if !is_master_control_note',
    'notify_board_event_direct(&ev)',
  ]);
  requireAll(diagnostics, FILES.slotHandler, s.slotHandler, [
    '"mission_master_status"',
    'master_control::mission_master_status(state).await',
    '"mcpApprovalReady"',
    '"mcpApprovalMissingTools"',
  ]);
  requireAll(diagnostics, FILES.masterControl, s.masterControl, [
    '"missiond.master-status.v2"',
    '"mcpReady"',
    '"mcpEnabled"',
    '"mcpApprovalReady"',
    '"missingTools"',
    '"eventCursor"',
    '"queuedEvents"',
    '"processedTicks"',
    '"driftBackfillTasksCreated"',
    '"lastDriftBackfillTaskId"',
    '"controlTurnsSent"',
    '"lastControlTurnError"',
    '"lastTickId"',
    '"lastMcpReady"',
    '"pty_recognition_snapshot"',
    'slot.config.project_root.clone().or(slot.config.cwd.clone())',
  ]);
  requireAll(diagnostics, FILES.mcpProcessTools, s.mcpProcessTools, [
    '"mission_master_status"',
    '常驻 Codex master-control 状态',
  ]);
  requireAll(diagnostics, FILES.mcpToolsMod, s.mcpToolsMod, [
    'test_master_status_surface_registered',
    'mission_master_status not registered',
  ]);
  requireAll(diagnostics, FILES.slotTypes, s.slotTypes, [
    'pub model_profile: Option<String>',
    'pub reasoning_effort: Option<String>',
    'pub search_enabled: Option<bool>',
    'pub sandbox: Option<String>',
    'pub approval_policy: Option<String>',
  ]);
  requireAll(diagnostics, FILES.ptyManager, s.ptyManager, [
    'pub reasoning_effort: Option<String>',
    'pub search_enabled: bool',
    'pub approval_policy: Option<String>',
  ]);
  requireAll(diagnostics, FILES.ptySession, s.ptySession, [
    'model_reasoning_effort',
    '--search',
    '--sandbox',
    '--ask-for-approval',
    'codex_command_projects_model_reasoning_search_sandbox_and_approval',
  ]);
  requireAll(diagnostics, FILES.frontendBlueprint, s.frontendBlueprint, [
    'codex-master-control',
    'Codex master and Gemini PTYs must be selectable exactly like ClaudeCode PTYs',
  ]);
  requireAll(diagnostics, FILES.aggregate, s.aggregate, [
    'scripts/check-v3-master-control-isomorphism.mjs',
  ]);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(`${file}: missing required text: ${needle}`);
  }
}

main();
