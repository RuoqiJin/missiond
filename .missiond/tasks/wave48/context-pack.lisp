(context-pack wave48-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave48
  :purpose "Parallel context investigation before code-shard implementation for dynamic slot restart recovery."
  :write-model append-only
  :sequence 4

  (observation :id wave48-context-bootstrap-001
    :agent codex-parent
    :seq 1
    :at "2026-04-29T06:01:00Z"
    :summary "Seed context-pack for two parallel ClaudeCode investigators. They should append observations and shard-proposals using scripts/context-pack-append.mjs, then the integrator will append integration-plan and dispatch code shards."
    :files [".missiond/v3/missiond-blueprint.lisp" "scripts/context-pack-append.mjs" "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"])

  (observation :id wave48-01-obs-restart-wipes-dyn-slot
    :agent claudecode-wave48-01
    :seq 2
    :at "2026-04-29T06:09:55Z"
    :task wave48-01-context-autopilot-restart-recovery-v0
    :summary "Daemon restart unconditionally wipes every dynamic slot. main.rs:252-269 (Phase 6.7) iterates list_dynamic_slots(active) and calls store.terminate_dynamic_slot(reason=daemon_restart) for each, with rationale 'Re-registering them creates zombie slots (DB active, process dead). Clean slate is safer.' SlotManager only knows static yaml slots after restart because compute_slot.rs:387 (state.mission.register_dynamic_slot) is the only registration site and it runs at create time. Meanwhile task_delegate.rs:217-221 persists the freshly-provisioned dyn slot id as BoardTask.assignee. Result: any pre-restart delegated BoardTask still pointing at a dyn slot id has lost both the runtime registration AND the DB row by the time autopilot tries to dispatch it post-restart."
    :files ["crates/missiond-daemon/src/main.rs" "crates/missiond-daemon/src/handlers/compute/compute_slot.rs" "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"])

  (observation :id wave48-01-obs-dispatch-trusts-stale-pin
    :agent claudecode-wave48-01
    :seq 3
    :at "2026-04-29T06:10:12Z"
    :task wave48-01-context-autopilot-restart-recovery-v0
    :summary "Autopilot dispatch trusts the stale pin without an existence check. autopilot.rs:471-503 reads task.assignee: if Some(id) it uses that id directly and never falls into the dynamic-assignment None branch. ensure_autopilot_pty (flow_engine.rs:535-602) then calls state.mission.list_slots().find(|s| s.config.id == slot_id), gets None for a dead dyn slot, records the '❌ Slot `X` 不存在' note, increments retry, and after max_retries marks the task failed at flow_engine.rs:570-593. recover_stale_running_tasks (board.rs:630-666) only resets status / claim_executor_id / lease — it never clears assignee — so the dead pin survives every recovery cycle. The whole burn-down is silent (failure note + status=failed); no Inbox alert, no jarvis notification."
    :files ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs" "crates/missiond-core/src/db/pg/board.rs"])

  (shard-proposal :id wave48-01-shard-clear-stale-dyn-pin
    :agent claudecode-wave48-01
    :seq 4
    :at "2026-04-29T06:10:30Z"
    :task wave48-01-context-autopilot-restart-recovery-v0
    :summary "Implementation shard: in dispatch_board_tasks, before honoring task.assignee=Some(id), check if state.mission.get_slot(id) is None AND store has no active dynamic_slot row for id. If so, clear the pin (BoardStore::clear_board_task_assignee) and let this tick re-route via the existing None-branch idle-coder selection. Add a board-task note ('🔄 Pinned slot X 在重启后已不可用，已解除 pin 等待重新调度') and skip this iteration. Adds: (1) BoardStore trait method clear_board_task_assignee(task_id) -> rows_affected with PG impl 'UPDATE board_tasks SET assignee = NULL, updated_at =  WHERE id =  AND status = open AND assignee = ' (idempotent CAS on the dead-pin id); (2) call site in autopilot.rs::dispatch_board_tasks immediately after the assignee match arm. No change to compute_slot.rs or flow_engine.rs; the existing 'no idle coder slot available, deferring' branch handles the case where no replacement slot exists. Touches a single owner (claudecode), no overlap with wave48-02 dispatch-shard-plan."
    :shard clear-stale-dyn-pin
    :owner claudecode
    :write-scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" "crates/missiond-core/src/db/traits.rs" "crates/missiond-core/src/db/pg/board.rs"]
    :must-not-touch ["crates/missiond-daemon/src/handlers/compute/**" "crates/missiond-daemon/src/main.rs" "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs" ".missiond/v3/missiond-blueprint.lisp" "scripts/**" "packages/**" ".missiond/tasks/wave48/manifest.lisp" ".missiond/tasks/wave48/wave48-*.lisp"]
    :acceptance ["cargo check -p missiond-daemon" "cargo test -p missiond-daemon --lib engine::intent_engine::autopilot" "cargo test -p missiond-core --lib db::pg::board"])
)
