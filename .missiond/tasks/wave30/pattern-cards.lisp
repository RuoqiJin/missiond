;; Wave 30 dispatch-time pattern cards.
;; Read-only guidance for workers. Stable cards remain under .missiond/patterns/.

(pattern-cards wave30-lifecycle-finalization
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave30

  (card lifecycle-finalizer
    :use-for [wave30-01-parent-hotfix-finalizer-v0]
    :recipe ["Treat worker draft report, parent patch commits, receipts, and final commit as separate facts."
             "Create a deterministic finalizer CLI with --dry-fixture and named exports; do not shell out for verification."
             "The final report :commit_hash should equal :final_commit_hash when parent patches exist; :agent_commit_hash preserves the worker commit."
             "A parent hotfix helper should append or consume lifecycle facts, then project report lineage; it should not require restarting the worker."]
    :known-good ["scripts/check-task-report.mjs"
                 "scripts/verify-task-run.mjs"
                 "scripts/verify-task-runner-batch.mjs"
                 ".missiond/tasks/wave29/reports/wave29-03-runner-wave-prep-v0.report.lisp"])

  (card staged-source-hygiene
    :use-for [wave30-02-staged-source-hygiene-v0]
    :recipe ["Bundle NUL byte detection, git diff --check style whitespace checks, and task-scope guard readiness into one read-only preflight."
             "Make the checker usable both from CLI and repo-local hook; default path must be read-only diagnostics."
             "Fixture binary/NUL cases using temporary files; do not introduce raw NUL bytes into repository source."
             "When integrating with hooks, preserve opt-in repo-local behavior and MISSIOND_TASK_CONTRACT gating."]
    :known-good ["scripts/task-scope-guard.mjs"
                 "scripts/check-missiond-hooks.mjs"
                 "scripts/install-missiond-hooks.mjs"
                 ".githooks/pre-commit"])

  (card lifecycle-event-log
    :use-for [wave30-03-atomic-lifecycle-event-log-v0]
    :recipe ["Define a small event schema with stable ids, task id, actor role, commit role, seq/timestamp, and repo-relative touched files."
             "Provide one append helper; workers/orchestrator call the helper instead of editing the shared ledger by hand."
             "Provide projection helpers back to shared-memory/session-trace so legacy validators keep working during migration."
             "Fixture concurrent seq behavior with deterministic temp files; no network, no git mutation, no LLM."]
    :known-good ["scripts/check-task-memory.mjs"
                 "scripts/check-session-trace.mjs"
                 "scripts/prepare-task-runner-wave.mjs"])

  (card manifest-hard-soft-deps
    :use-for [wave30-04-manifest-hard-soft-deps-v2]
    :recipe ["Separate hard dependencies used for dispatch from soft references used for brief context."
             "Keep v1 manifest compatibility; v2 fields should be additive or gated by a schema version."
             "Ready-queue must release tasks as soon as hard deps are satisfied; soft refs must not create barriers."
             "Renderer should display soft refs as context, not as blockers."]
    :known-good ["scripts/check-task-runner-manifest.mjs"
                 "scripts/plan-task-runner.mjs"
                 "scripts/render-wave-briefs.mjs"
                 ".missiond/tasks/wave29/manifest.lisp"])

  (card lifecycle-receipt-smoke
    :use-for [wave30-05-lifecycle-receipt-smoke-v0]
    :recipe ["Add layer-local fixtures first, then one synthetic wave that crosses event append, finalization, receipt coverage, ready-queue, and batch verification."
             "Use true disk artifacts where possible so drift is caught by future runs."
             "Each failing invariant should point at the nearest owning layer; avoid one opaque mega-runner."
             "Audit all touched scripts for raw NUL bytes before commit."]
    :known-good ["scripts/check-verification-receipt.mjs"
                 "scripts/verify-task-runner-batch.mjs"
                 "scripts/plan-task-runner.mjs"
                 ".missiond/patterns/cross-layer-smoke.pattern.lisp"]))

