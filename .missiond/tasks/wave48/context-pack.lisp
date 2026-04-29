(context-pack wave48-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave48
  :purpose "Parallel context investigation before code-shard implementation for dynamic slot restart recovery."
  :write-model append-only
  :sequence 11

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

  (observation :id wave48-02-obs-ttl-same-trap
    :agent claudecode-wave48-02
    :seq 5
    :at "2026-04-29T06:18:38Z"
    :task wave48-02-context-dispatch-shard-plan-v0
    :summary "TTL reaping (reap_expired_dynamic_slots autopilot.rs:1505-1532) and daemon-restart wipe (main.rs:252-269 Phase 6.7) produce the IDENTICAL dangling-pin condition — both call store.terminate_dynamic_slot + state.mission.unregister_dynamic_slot on the dyn slot, leaving any open BoardTask whose assignee equals that slot id with a runtime-missing pin. wave48-01's dispatch-side clear (state.mission.get_slot existence check inside dispatch_board_tasks before honoring task.assignee) covers BOTH cases atomically: the trigger is dispatch-time presence, not the death cause. Implication: a separate 'clear pins on slot terminate' shard would mutate autopilot.rs + db/pg/board.rs which wave48-01-shard-clear-stale-dyn-pin already owns — that shard split would be a write-scope overlap. Recommend keeping the cleanup centralised inside the wave48-01 dispatch path."
    :files ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" "crates/missiond-daemon/src/main.rs"])

  (observation :id wave48-02-obs-blueprint-checker-coupling
    :agent claudecode-wave48-02
    :seq 6
    :at "2026-04-29T06:18:52Z"
    :task wave48-02-context-dispatch-shard-plan-v0
    :summary "Workstation-config isomorphism is a single-owner hotspot. scripts/check-v3-workstation-config-isomorphism.mjs requireAll (line 162-177) projects blueprint invariant text into autopilot.rs source. Today the blueprint declares 'Autopilot pty.send budget MUST project from BoardTask.timeout_secs' and 'The per-slot dispatch guard MUST be held across the entire state.pty.send call'. wave48-01-shard-clear-stale-dyn-pin's write-scope covers autopilot.rs + db/traits.rs + db/pg/board.rs but NOT the blueprint or its checker. Consequence: the new 'stale-dyn-pin clear' contract is not captured in the V3 surface. Either expand wave48-01 to atomically include .missiond/v3/missiond-blueprint.lisp + scripts/check-v3-workstation-config-isomorphism.mjs, OR dispatch a sibling shard that lands in lockstep. Splitting blueprint and autopilot.rs across two non-coupled shards risks the checker passing today but the invariant drifting silently later — that is the exact 'V3 drift' wave47 already pinned for workstation_dispatch."
    :files [".missiond/v3/missiond-blueprint.lisp" "scripts/check-v3-workstation-config-isomorphism.mjs" "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"])

  (observation :id wave48-02-obs-smoke-coverage-gap
    :agent claudecode-wave48-02
    :seq 7
    :at "2026-04-29T06:19:10Z"
    :task wave48-02-context-dispatch-shard-plan-v0
    :summary "No live-smoke proves the restart-recovery path. wave47-01 (commit 75f0791ce096) added scripts/check-v3-request-flow-smoke.mjs --execute-real-dispatch which only proves the happy-path delegate succeeds (delegated_board_task_id surfaces, status=executing). The wave47 report's Operational note literally documents the failure mode wave48 targets: 'the initial ClaudeCode worker lost its dynamic slot when the daemon had to be restarted to install the new workstation_dispatch projection. Parent took over without discarding worker edits.' That manual recovery is exactly what wave48-01's clear-stale-dyn-pin should automate. A new --restart-during-dispatch sibling mode (or a new sibling smoke) that delegates a long task, kills the daemon mid-flight, restarts, and asserts the autopilot tick clears the dead-pin and re-dispatches to a different slot id is write-scope-disjoint from wave48-01: it touches only scripts/check-v3-request-flow-smoke.mjs (no overlap with autopilot.rs, board.rs, traits.rs, blueprint, or workstation-config checker). This makes a clean second-worker shard once wave48-01 has merged."
    :files ["scripts/check-v3-request-flow-smoke.mjs" ".missiond/tasks/wave47/reports/wave47-01-request-real-dispatch-smoke-v0.report.lisp" "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"])

  (shard-proposal :id wave48-02-shard-blueprint-checker-pin
    :agent claudecode-wave48-02
    :seq 8
    :at "2026-04-29T06:19:26Z"
    :task wave48-02-context-dispatch-shard-plan-v0
    :summary "Single-owner hotspot. Adds a workstation-config invariant pinning 'Autopilot MUST clear BoardTask.assignee when the pinned dynamic slot is no longer registered (post-restart or post-TTL)' into .missiond/v3/missiond-blueprint.lisp, and a matching requireAll string in scripts/check-v3-workstation-config-isomorphism.mjs that the wave48-01 autopilot.rs change introduces (e.g., a stable identifier comment 'clear-stale-dyn-pin'). Write-scope is disjoint from wave48-01-shard-clear-stale-dyn-pin (crates/** + db/traits.rs + db/pg/board.rs), and disjoint from wave48-02-shard-recovery-smoke (scripts/check-v3-request-flow-smoke.mjs only), so the three shards can be accepted together by an integration-plan with NO write-scope overlap. MUST land in the SAME merge as wave48-01 to avoid checker-source drift; the integrator should put both into dispatch-group A and either dispatch them to the same claudecode worker sequentially or merge them into a single PR before promoting."
    :shard blueprint-checker-pin
    :owner claudecode
    :write-scope [".missiond/v3/missiond-blueprint.lisp" "scripts/check-v3-workstation-config-isomorphism.mjs"]
    :must-not-touch ["crates/**" "packages/**" "scripts/check-v3-request-flow-smoke.mjs" "scripts/check-context-pack.mjs" "scripts/context-pack-append.mjs" ".missiond/v1/**" ".missiond/v2/**" ".missiond/tasks/wave48/manifest.lisp" ".missiond/tasks/wave48/wave48-*.lisp"]
    :acceptance ["node scripts/check-v3-workstation-config-isomorphism.mjs" "node scripts/check-v3-context-pack-isomorphism.mjs" "node scripts/check-v3-code-isomorphism-complete.mjs" "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp" "node scripts/check-lisp-blueprint-compression.mjs"])

  (shard-proposal :id wave48-02-shard-recovery-smoke
    :agent claudecode-wave48-02
    :seq 9
    :at "2026-04-29T06:19:46Z"
    :task wave48-02-context-dispatch-shard-plan-v0
    :summary "Live-smoke shard for the restart-recovery contract. Extends scripts/check-v3-request-flow-smoke.mjs with an opt-in --restart-during-dispatch sub-mode (mirroring wave47-01's --execute-real-dispatch opt-in pattern): delegate a long-timeout dev task, await delegated_board_task_id + assignee=slot-dyn-XXX, send SIGTERM to the daemon, wait for daemon restart (launchctl-managed), assert (a) state.mission.list_dynamic_slots(active) returns empty for the original slot id post-restart, (b) within N autopilot ticks the BoardTask.assignee for the queued task transitions Some(slot-dyn-XXX) -> None and is then re-dispatched to a fresh idle coder slot id, (c) pipeline_result.status moves through 'queued'/'running' to 'done' with no human intervention. Write-scope is scripts/check-v3-request-flow-smoke.mjs ONLY: disjoint from wave48-01-shard-clear-stale-dyn-pin (crates/** + db) AND from wave48-02-shard-blueprint-checker-pin (.missiond/v3/missiond-blueprint.lisp + scripts/check-v3-workstation-config-isomorphism.mjs). dispatch-group: B — depends-on wave48-01 + wave48-02-shard-blueprint-checker-pin landing first, because the smoke can only pass once the autopilot fix and V3 invariant pin are both merged."
    :shard recovery-smoke
    :owner claudecode
    :write-scope ["scripts/check-v3-request-flow-smoke.mjs"]
    :must-not-touch ["crates/**" "packages/**" ".missiond/v1/**" ".missiond/v2/**" ".missiond/v3/**" "scripts/check-v3-workstation-config-isomorphism.mjs" "scripts/check-context-pack.mjs" "scripts/context-pack-append.mjs" ".missiond/tasks/wave48/manifest.lisp" ".missiond/tasks/wave48/wave48-*.lisp"]
    :acceptance ["node scripts/check-v3-request-flow-smoke.mjs --dry-fixture" "node scripts/check-v3-request-flow-smoke.mjs" "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --execute-real-dispatch --restart-during-dispatch --cleanup --json" "node scripts/check-v3-code-isomorphism-complete.mjs"])

  (conflict :id wave48-02-conflict-terminate-side-cleanup
    :agent claudecode-wave48-02
    :seq 10
    :at "2026-04-29T06:20:03Z"
    :task wave48-02-context-dispatch-shard-plan-v0
    :summary "Hypothetical 'clear assignees on dynamic-slot terminate' shard would touch reap_expired_dynamic_slots (autopilot.rs:1505-1532), the spawn-failed branch (compute_slot.rs:463-466) and the user-terminated branch (compute_slot.rs:509-521) plus a new BoardStore::clear_assignees_for_slot call in db/pg/board.rs. That write-scope overlaps wave48-01-shard-clear-stale-dyn-pin (autopilot.rs + db/pg/board.rs) and adds compute_slot.rs which wave48-01 explicitly excludes via its must-not-touch list. Two shards mutating autopilot.rs in parallel is a hard merge conflict and a checker-isomorphism risk. Resolution: do NOT spawn this shard. wave48-01's dispatch-side existence check (state.mission.get_slot(id) + clear_board_task_assignee) covers the same dangling-pin condition for both restart-wipe AND TTL-reap (see observation wave48-02-obs-ttl-same-trap), so the centralised dispatch-time fix is strictly preferred over a multi-site terminate-time cleanup."
    :files ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" "crates/missiond-daemon/src/handlers/compute/compute_slot.rs" "crates/missiond-core/src/db/pg/board.rs"]
    :shards [clear-stale-dyn-pin terminate-side-cleanup-hypothetical])

  (integration-plan :id wave48-integration-plan-001
    :agent codex-integrator
    :seq 11
    :at "2026-04-29T06:26:39Z"
    :summary "Accepted wave48 shards after parallel investigation. Group A has landed: clear-stale-dyn-pin plus blueprint-checker-pin in commit 5c3f30d3. Group B remains: recovery-smoke should add an opt-in restart-during-dispatch smoke without touching crates or V3 blueprint/checker files."
    :files [".missiond/tasks/wave48/context-pack.lisp" "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" "scripts/check-v3-request-flow-smoke.mjs"]
    :accepted-shards [clear-stale-dyn-pin blueprint-checker-pin recovery-smoke]
    :dispatch-groups [A B])
)
