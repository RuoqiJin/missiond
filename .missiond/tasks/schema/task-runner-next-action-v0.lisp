;; MissionD task-runner-next-action v0
;; Purpose: a small controller surface that turns task-runner-wave-state
;; projections into the next orchestrator action.

(task-runner-next-action-schema missiond.task-runner-next-action.v0
  :version "v0"
  :status "code-aligned -- scripts/task-runner-next-action.mjs selects runnable actions and can explicitly emit dispatch lifecycle events"
  :checker "scripts/task-runner-next-action.mjs --dry-fixture"

  (purpose
    "Bridge read-only wave-state projection and a daemon/MCP dispatcher wrapper."
    "Default to read-only selection so callers can inspect the next action without mutation."
    "Make dispatch recording explicit and lifecycle-backed before any future worker spawn integration.")

  (inputs
    (:manifest "task-runner-manifest v1/v2; required")
    (:lifecycle "task-lifecycle-event v1 ledger; optional; same default convention as task-runner-wave-state")
    (:receipts "verification-receipt v1 file; optional")
    (:selection-policy "runnable | all | dispatch_task | finalize_report | wait_for_hard_deps")
    (:limit "non-negative integer or all"))

  (selection-policy
    (runnable "Select finalize_report actions first; if none, select all dispatch_task actions; if none, surface wait_for_hard_deps.")
    (all "Return every wave-state next_actions entry in priority order.")
    (dispatch_task "Return dispatchable tasks only.")
    (finalize_report "Return finalization-required tasks only.")
    (wait_for_hard_deps "Return blocked wait explanations only."))

  (mutation-boundary
    "Default mode is read-only: no git, no spawn, no network, no LLM, no writes."
    "--emit-dispatch-events is the only mutation mode."
    "--emit-dispatch-events is legal only when every selected action is dispatch_task."
    "Mutation writes event_kind=dispatch into .missiond/tasks/<wave>/task-lifecycle-events.lisp through scripts/task-runner-append-event.mjs."
    "After mutation the command re-projects wave state so dispatched tasks move out of dispatchable and into running.")

  (output-json
    "{ ok, schema, mutation_mode, selection_policy, limit, wave, manifest_path, lifecycle_path, counts, selected_count, selected_actions[], appended_events[], after_counts?, after_running?, after_dispatchable? }")

  (non-goals
    "Does not spawn ClaudeCode workers."
    "Does not finalize reports."
    "Does not mutate git."
    "Does not replace the daemon; it is the deterministic CLI surface the daemon can wrap."))
