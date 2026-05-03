#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-autopilot-runtime-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 autopilot-runtime Lisp/code isomorphism contract:
  - Autopilot is an event-driven delegated BoardTask runtime, not merely a
    workstation-config note.
  - BoardEvent/SlotEvent subscribers wake the dedicated Autopilot task and ack
    immediately; they never run pty.send inline.
  - mission_task_delegate emits BoardEvent::TaskCreated so real dispatch has a
    bus-visible causal edge before Autopilot picks it up.
`;

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  subscribers: 'crates/missiond-daemon/src/bus/v2_subscribers.rs',
  taskDelegate: 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  boardEvents: 'crates/missiond-daemon/src/handlers/knowledge/board/events.rs',
  boardEvent: 'crates/missiond-core/src/event/events/board.rs',
  slotEvent: 'crates/missiond-core/src/event/events/slot.rs',
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
  const diagnostics = checkFiles(repoRoot);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(FILES).length,
    diagnostics,
  };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 autopilot-runtime Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(
      `v3 autopilot-runtime Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }
  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(path.join(root, rel), 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, FILES.blueprint, sources.blueprint, [
    '(event-causality-runtime',
    'MissionD nervous-system contract',
    '(state-machine delegated-boardtask-runtime',
    '(function delegated-boardtask-runtime',
    '(surface autopilot-runtime',
    ':status "code-aligned"',
    'BoardEvent::TaskCreated',
    'BoardEvent::Updated(status=open)',
    'BoardEvent::StatusChanged(new_status=open)',
    'SlotEvent::BecameIdle',
    'autopilot-runtime.dispatch-boardtask',
    'Subscribers only notify the dedicated Autopilot task and ack immediately',
    'crates/missiond-daemon/src/bus/v2_subscribers.rs',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    'node scripts/check-v3-autopilot-runtime-isomorphism.mjs',
    // Summary-note pollution invariant: raw res.response forbidden as note source.
	    ':summary-note-source',
	    ':settle-window',
	    ':idle-durable-summary-close',
	    'extract_worker_final_summary(res.response, full_prompt)',
	    'wait_for_worker_final_settle_window',
	    'durable provider-or-note evidence + idle slot diagnosis',
	    'paste again to expand',
	  ]);

  requireAll(diagnostics, FILES.subscribers, sources.subscribers, [
    'spawn_autopilot_board_event_sub',
    'spawn_autopilot_slot_event_sub',
    'v2_autopilot_board_event',
    'v2_autopilot_slot_event',
    'board_event_should_wake_autopilot',
    'slot_event_should_wake_autopilot',
    'BoardEvent::TaskCreated',
    'BoardEvent::Updated { status, .. }',
    'BoardEvent::StatusChanged { new_status, .. }',
    'SlotEvent::BecameIdle',
    'state.board_dispatch_notify.notify_one()',
    'ack.ack().await',
    'it never runs `dispatch_board_tasks` inline',
  ]);
  forbidAll(diagnostics, FILES.subscribers, sources.subscribers, [
    'autopilot::dispatch_board_tasks(&state).await',
  ]);

  requireAll(diagnostics, FILES.taskDelegate, sources.taskDelegate, [
    'missiond_core::event::events::BoardEvent',
    'BoardEvent::TaskCreated',
    'notify_board_event_direct(&ev)',
    'state.bus.publish_board(ev).await',
    'event-bus cause',
    'state.board_dispatch_notify.notify_one()',
  ]);

  requireAll(diagnostics, FILES.autopilot, sources.autopilot, [
    'dispatch_board_tasks',
    'OwnedSlotDispatchGuard',
    'state.pty.send(&slot_id, &full_prompt, timeout_ms).await',
    'maybe_complete_delegated_execution_log',
    'publish_slot(SlotEvent::TaskDispatched',
    // Summary-note pollution: extractor + cap MUST be wired into the
    // success path; legacy `res.duration_ms, res.response` pairing is
    // forbidden so a refactor cannot regress to the polluted note shape.
    'fn extract_worker_final_summary',
	    'AUTOPILOT_SUMMARY_NOTE_MAX_BYTES',
	    'AUTOPILOT_FINAL_SETTLE_WINDOW_MS_DEFAULT',
	    'fn worker_final_settle_window_ms',
    'wait_for_worker_final_settle_window().await',
    'close_idle_running_task_from_durable_summary',
    'has_durable_completion_summary_after_claim',
    'durable_provider_completion_for_slot_task',
    'latest_assistant_after_task_prompt',
    'get_conversations_by_task_id',
    'get_slot_session',
    'set_conversation_task_id',
    'Provider durable final observed',
    'is_durable_completion_summary_note',
    'get_board_task_with_notes',
    'extract_worker_final_summary(&res.response, &full_prompt)',
	    'truncate_safe(&final_summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES)',
	  ]);
  forbidAll(diagnostics, FILES.autopilot, sources.autopilot, [
    'res.duration_ms, res.response',
  ]);

  requireAll(diagnostics, FILES.boardEvents, sources.boardEvents, [
    'publish_board_created',
    'BoardEvent::TaskCreated',
    'bus.publish_board(ev).await',
  ]);
  requireAll(diagnostics, FILES.boardEvent, sources.boardEvent, [
    'pub enum BoardEvent',
    'TaskCreated',
    'StatusChanged',
    'Updated',
  ]);
  requireAll(diagnostics, FILES.slotEvent, sources.slotEvent, [
    'pub enum SlotEvent',
    'BecameIdle',
    'TaskDispatched',
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
      diagnostics.push({ file, message: `forbidden text present: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-autopilot-runtime-'));
  for (const rel of Object.values(FILES)) {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  }
  write(root, 'blueprint', `
(missiond-blueprint
  (event-causality-runtime
    :desc "MissionD nervous-system contract"
    :autopilot-trigger-contract "BoardEvent::TaskCreated, BoardEvent::Updated(status=open), BoardEvent::StatusChanged(new_status=open), and SlotEvent::BecameIdle MUST wake Autopilot through event-bus subscribers. Subscribers only notify the dedicated Autopilot task and ack immediately."
    :chain [autopilot-runtime.dispatch-boardtask])
  (state-machines (state-machine delegated-boardtask-runtime))
  (pillar-flow-map
    (pillar workstation
      (function delegated-boardtask-runtime
        :surface autopilot-runtime
        :entry [BoardEvent.TaskCreated SlotEvent.BecameIdle]
        :core ((step s1 :logic "x"))
        :egress [BoardEvent SlotEvent])))
	  (execution-ownership delegated-boardtask
	    :close-owner
	      (:summary-note-source "extract_worker_final_summary(res.response, full_prompt) capped via truncate_safe; raw res.response forbidden in note format string. Auth/quota notes bypass intentionally. The TUI capture includes echoed task contract, tool logs, and \`paste again to expand\` collapse markers."
	       :settle-window "wait_for_worker_final_settle_window projects the high-confidence final summary settle policy."
	       :idle-durable-summary-close "delayed active-frame tasks close from durable provider-or-note evidence + idle slot diagnosis."))
  (implementation-map
    (surface autopilot-runtime
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/bus/v2_subscribers.rs" "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"]
      :note "n"))
  (compression-contract
    :checks ["node scripts/check-v3-autopilot-runtime-isomorphism.mjs"]))`);
  write(root, 'subscribers', `
use missiond_core::event::events::{BoardEvent, SlotEvent};
fn spawn_autopilot_board_event_sub() { let _ = "v2_autopilot_board_event"; state.board_dispatch_notify.notify_one(); ack.ack().await; }
fn spawn_autopilot_slot_event_sub() { let _ = "v2_autopilot_slot_event"; state.board_dispatch_notify.notify_one(); ack.ack().await; }
fn board_event_should_wake_autopilot(e: &BoardEvent) -> bool { match e { BoardEvent::TaskCreated { .. } => true, BoardEvent::Updated { status, .. } => status.eq_ignore_ascii_case("open"), BoardEvent::StatusChanged { new_status, .. } => new_status.eq_ignore_ascii_case("open"), _ => false } }
fn slot_event_should_wake_autopilot(e: &SlotEvent) -> bool { matches!(e, SlotEvent::BecameIdle { .. }) }
// it never runs \`dispatch_board_tasks\` inline
`);
  write(root, 'taskDelegate', `
use missiond_core::event::events::BoardEvent;
async fn x() { let ev = BoardEvent::TaskCreated { task_id, title, category }; notify_board_event_direct(&ev); let _ = state.bus.publish_board(ev).await; state.board_dispatch_notify.notify_one(); /* event-bus cause */ }`);
  write(root, 'autopilot', `
	const AUTOPILOT_SUMMARY_NOTE_MAX_BYTES: usize = 4000;
	const AUTOPILOT_FINAL_SETTLE_WINDOW_MS_DEFAULT: u64 = 1200;
	fn worker_final_settle_window_ms() -> u64 { AUTOPILOT_FINAL_SETTLE_WINDOW_MS_DEFAULT }
		fn extract_worker_final_summary(_r: &str, _p: &str) -> String { String::new() }
		fn is_durable_completion_summary_note() {}
		fn has_durable_completion_summary_after_claim() {}
    fn latest_assistant_after_task_prompt() {}
    async fn durable_provider_completion_for_slot_task() { get_conversations_by_task_id(); get_slot_session(); set_conversation_task_id(); }
		async fn close_idle_running_task_from_durable_summary() { get_board_task_with_notes(); let _ = "Provider durable final observed"; }
		async fn dispatch_board_tasks() {
	    let _g = OwnedSlotDispatchGuard;
	    state.pty.send(&slot_id, &full_prompt, timeout_ms).await;
	    wait_for_worker_final_settle_window().await;
	    let final_summary = extract_worker_final_summary(&res.response, &full_prompt);
    let _summary = truncate_safe(&final_summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES);
    maybe_complete_delegated_execution_log();
    publish_slot(SlotEvent::TaskDispatched{});
}`);
  write(root, 'boardEvents', `
fn publish_board_created() { let ev = BoardEvent::TaskCreated {}; bus.publish_board(ev).await; }`);
  write(root, 'boardEvent', `
pub enum BoardEvent { TaskCreated, StatusChanged, Updated }`);
  write(root, 'slotEvent', `
pub enum SlotEvent { BecameIdle, TaskDispatched }`);
  return root;
}

function write(root, key, source) {
  fs.writeFileSync(path.join(root, FILES[key]), source.trimStart());
}

main();
