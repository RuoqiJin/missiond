;; MissionD task-runner-wave-state v0
;; Purpose: read-only orchestration projection that lets MissionD decide what
;; to dispatch or finalize next from Lisp artifacts.

(task-runner-wave-state-schema missiond.task-runner-wave-state.v0
  :version "v0"
  :status "code-aligned — scripts/task-runner-wave-state.mjs projects manifest + finalized reports + optional lifecycle events + optional receipts into deterministic task states"
  :checker "scripts/task-runner-wave-state.mjs --dry-fixture"

  (purpose
    "Bridge the gap between Lisp artifacts and actual MissionD work scheduling."
    "Make the runner answer machine questions: which tasks are complete, dispatchable, blocked, running, or need finalization."
    "Keep dispatch decisions based on hard dependencies and finalized reports, while soft references remain context only.")

  (inputs
    (:manifest "task-runner-manifest v1/v2; required")
    (:lifecycle "task-lifecycle-event v1 ledger; optional; defaults to .missiond/tasks/<wave>/task-lifecycle-events.lisp when present")
    (:reports ".missiond/tasks/<wave>/reports/<task>.report.lisp finalized reports; read by convention")
    (:receipts "verification-receipt v1 file; optional"))

  (states
    (complete "A finalized/done report exists with a final commit hash and no later unfinalized parent_hotfix event is observed.")
    (dispatchable "All effective hard deps are complete; task has no report and is not running.")
    (blocked "At least one effective hard dep is not complete.")
    (running "Lifecycle events show dispatch/claim/trace_start/read but no final report yet.")
    (needs_finalization "Worker commit or later parent_hotfix exists but the finalized report does not include that final lineage."))

  (dependency-contract
    "Effective hard deps use task-runner-manifest v2 :hard_deps when declared, otherwise v1 :depends_on."
    "Soft refs are projected for context but never appear in pending_hard_deps and never block dispatch.")

  (output-json
    "{ ok, schema, manifest_path, lifecycle_path, receipts_path, wave, counts, next_actions[], dispatchable[], needs_finalization[], running[], blocked[], complete[], ready_queue_order[], tasks[] }"
    "Each task row carries task_id, state, hard_deps, soft_refs, pending_hard_deps, dispatch_group, verification_tier, report_path, final_commit_hash, agent_commit_hash, latest_parent_hotfix_hash, lifecycle_event_count, receipt_count, reusable_receipt_count.")

  (next-actions
    (finalize_report "Emitted before dispatch actions when worker commits or parent_hotfix events are not reflected in a final report.")
    (dispatch_task "Emitted for dispatchable tasks with all hard deps complete; includes brief_path plus soft_refs for context.")
    (wait_for_hard_deps "Emitted for blocked tasks with the exact pending_hard_deps list."))

  (non-goals
    "Does not spawn workers."
    "Does not mutate git or inspect git history."
    "Does not write reports, receipts, ledgers, briefs, or archives."
    "Does not replace batch verification; it is the scheduling/status projection consumed before dispatch."))
