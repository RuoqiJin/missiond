;; MissionD task-runner-dispatch v0
;; Purpose: convert Lisp task-runner next actions into existing MCP
;; mission_task_delegate call descriptors without re-implementing dispatch in
;; the daemon.

(task-runner-dispatch-schema missiond.task-runner-dispatch.v0
  :version "v0"
  :status "code-aligned -- scripts/task-runner-dispatch.mjs builds mission_task_delegate descriptors from task-runner-next-action output"
  :checker "scripts/task-runner-dispatch.mjs --dry-fixture"

  (purpose
    "Make Lisp manifests and lifecycle state drive the next worker dispatch without requiring a human to translate briefs into MCP calls."
    "Keep the canonical orchestration decision in Node/Lisp CLIs so a future daemon wrapper can stay thin."
    "Reuse mission_task_delegate rather than introducing a new worker-spawn substrate.")

  (inputs
    (:manifest "task-runner-manifest v1/v2; required")
    (:lifecycle "task-lifecycle-event v1 ledger; optional")
    (:receipts "verification-receipt v1 file; optional")
    (:max-parallel "non-negative integer or all; passed to task-runner-next-action limit")
    (:allow-missing-briefs "default false; when false, missing .missiond/claudecode/<task>.md blocks delegation")
    (:emit-dispatch-events "default false; explicit mutation mode that appends lifecycle dispatch events after descriptor readiness checks"))

  (selection
    "Calls task-runner-next-action with selection policy runnable."
    "If finalize_report actions are selected, status is blocked_by_finalization and no delegate calls are emitted."
    "If wait_for_hard_deps actions are selected, status is blocked_by_hard_deps and no delegate calls are emitted."
    "If dispatch_task actions are selected and every brief exists, status is ready_to_delegate.")

  (delegate-call-shape
    "{ task_id, target_tool: mission_task_delegate, target_args: { objective, intent, cwd, timeout_secs, priority, context_hints[] }, dispatch_event }"
    "objective points the worker at the thin brief, task contract, wave manifest, shared protocol, and soft context refs."
    "timeout_secs is deterministic from estimated_minutes plus padding, capped to mission_task_delegate's 7200 second maximum."
    "context_hints include the task id, wave id, brief path, contract path, manifest path, and soft refs.")

  (mutation-boundary
    "Default mode is read-only: no git, no spawn, no network, no LLM, no MCP call, no writes."
    "--emit-dispatch-events delegates event writing to task-runner-next-action after descriptor readiness has passed."
    "The CLI still does not call mission_task_delegate; it returns call descriptors for a daemon/MCP wrapper.")

  (output-json
    "{ ok, schema, mutation_mode, wave, manifest_path, lifecycle_path, max_parallel, status, counts, selected_actions[], blocker_actions[], missing_briefs[], delegate_call_count, delegate_calls[], appended_events[], after_counts?, after_running?, after_dispatchable? }")

  (non-goals
    "Does not spawn ClaudeCode workers directly."
    "Does not call mission_task_delegate itself."
    "Does not finalize reports."
    "Does not mutate git."
    "Does not replace task-runner-next-action; it packages selected dispatch actions for the existing MissionD task delegation substrate."))
