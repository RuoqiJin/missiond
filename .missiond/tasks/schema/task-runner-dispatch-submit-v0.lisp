;; MissionD task-runner-dispatch-submit v0
;; Purpose: explicitly apply task-runner-dispatch descriptors to the running
;; missiond daemon IPC tools/call endpoint.

(task-runner-dispatch-submit-schema missiond.task-runner-dispatch-submit.v0
  :version "v0"
  :status "code-aligned -- scripts/task-runner-submit-dispatch.mjs dry-runs by default and requires --apply before calling daemon IPC"
  :checker "scripts/task-runner-submit-dispatch.mjs --dry-fixture"

  (purpose
    "Remove the human-as-transport gap between Lisp runner state and mission_task_delegate."
    "Keep dispatch selection in task-runner-dispatch.mjs, then submit its delegate_calls through the existing missiond tools/call IPC protocol."
    "Record lifecycle dispatch events only for delegate calls whose daemon submission succeeds.")

  (inputs
    (:manifest "task-runner-manifest v1/v2; required")
    (:lifecycle "task-lifecycle-event v1 ledger; optional")
    (:receipts "verification-receipt v1 file; optional")
    (:endpoint "MISSION_IPC_ENDPOINT or MISSION_IPC_SOCKET, default ~/.missiond/missiond.sock")
    (:session-id "CLAUDE_SESSION_ID or SESSION_ID, default task-runner-<pid>")
    (:apply "default false; required for daemon IPC mutation"))

  (mutation-boundary
    "Default mode is dry-run: no IPC connection, no lifecycle append, no spawn, no git, no network outside local daemon socket."
    "--apply submits only the descriptor's delegate_calls to mission_task_delegate via tools/call."
    "A successful daemon response appends event_kind=dispatch for that task through task-runner-next-action's event writer."
    "Failed submissions are counted and do not receive dispatch events.")

  (output-json
    "{ ok, schema, mode, endpoint?, session_id?, wave, manifest_path, lifecycle_path, dispatch_status, delegate_call_count, submitted_count, failed_count, descriptor, submissions[], appended_events[], after_counts?, after_running?, after_dispatchable? }")

  (non-goals
    "Does not introduce a new MCP tool."
    "Does not bypass mission_task_delegate."
    "Does not auto-start missiond; daemon reachability remains an explicit runtime prerequisite."
    "Does not call ClaudeCode directly or shell out to claude."))
