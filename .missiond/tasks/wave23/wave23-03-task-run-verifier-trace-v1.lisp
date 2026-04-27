;; Wave 23 task contract.

(task wave23-03-task-run-verifier-trace-v1
  :schema "missiond.task-contract.v1"
  :title "Task run verifier trace integration v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave23-01-session-trace-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Teach the task-run verifier to include session-trace checks so completed tasks have contract/report/memory/commit/trace proof in one read-only verifier."

  :write-scope
    ["scripts/verify-task-run.mjs"
     "scripts/check-session-trace.mjs"
     "scripts/check-task-memory.mjs"
     "scripts/check-task-report.mjs"
     "scripts/lib/missiond_lisp.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/schema/*.lisp"
     ".missiond/tasks/wave22/**"]

  :requirements
    ["Add --trace <session-trace.lisp> to scripts/verify-task-run.mjs."
     "Verify the trace contains at least one completion event for the task when --trace is supplied."
     "Verify commit_hash in trace completion matches report commit_hash when both are present."
     "Preserve existing behavior when --trace is absent unless --require-trace is provided."
     "Add --dry-fixture coverage for pass, missing completion, mismatched commit, malformed trace, and absent trace allowed."]

  :acceptance
    ["node scripts/verify-task-run.mjs --dry-fixture"
     "node scripts/check-session-trace.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/verify-task-run.mjs scripts/check-session-trace.mjs scripts/check-task-memory.mjs scripts/check-task-report.mjs scripts/lib/missiond_lisp.mjs"]

  :commit
    (:required true
     :message "feat(tasks): verify task runs with session trace"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New verifier flags."
     "Trace checks."
     "Acceptance command results."])
