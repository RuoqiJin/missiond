;; Wave 29 task contract.

(task wave29-05-verification-receipt-schema-v0
  :schema "missiond.task-contract.v1"
  :title "Verification receipt schema v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave29-04-parent-hotfix-lineage-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "B"
  :estimated-minutes 40
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave29/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave29/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Introduce verification receipts so the orchestrator can reuse already-run smoke/full evidence across a wave instead of blindly repeating expensive checks. Receipts cache command evidence; they are never a substitute for source facts or commit verification."

  :write-scope
    [".missiond/tasks/schema/verification-receipt-v1.lisp"
     "scripts/check-verification-receipt.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/schema/context-atlas-v1.lisp"
     ".missiond/tasks/schema/pattern-card-v1.lisp"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/wave29-*.lisp"
     ".missiond/tasks/wave29/manifest.lisp"
     ".missiond/tasks/wave29/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/check-context-atlas.mjs"
     "scripts/check-pattern-card.mjs"
     "scripts/check-task-report.mjs"
     "scripts/verify-task-run.mjs"
     "scripts/prepare-task-runner-wave.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/plan-task-runner.mjs"]

  :requirements
    ["Define schema missiond.verification-receipt.v1 with wave, task_id, commit_hash, command, exit_code, started_at/finished_at or duration_ms, tier, and files/paths evidence."
     "Checker must validate command strings, positive/non-negative durations, exit_code integer, tier enum local|smoke|full, commit hash shape, repo-relative paths, duplicate receipt ids, and stale wave/task mismatch."
     "verify-task-runner-batch may load optional receipts and report receipt coverage, but must still verify task contract, report, memory completion, and git commit."
     "Receipt reuse rules must be conservative: wrong commit, wrong command, non-zero exit, or stale tier must not count as reusable evidence."
     "Checker supports --json, --stdin, --dry-fixture; no git mutation, no network, no LLM."
     "Fixtures must include valid smoke receipt, stale commit rejection, wrong tier rejection, non-zero exit rejection, duplicate id rejection, and batch verifier coverage."]

  :acceptance
    ["node scripts/check-verification-receipt.mjs --dry-fixture"
     "node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/verification-receipt-v1.lisp scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add verification receipt checks"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Receipt schema fields."
     "Conservative reuse rules."
     "Acceptance command results."])
