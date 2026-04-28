;; Wave 29 task contract.

(task wave29-07-runner-efficiency-smoke-v1
  :schema "missiond.task-contract.v1"
  :title "Runner efficiency smoke v1"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave29-03-runner-wave-prep-v0"
               "wave29-04-parent-hotfix-lineage-v1"
               "wave29-05-verification-receipt-schema-v0"
               "wave29-06-ready-queue-planner-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier smoke
  :dispatch-group "C"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave29/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave29/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Add a cross-layer smoke suite for runner-efficiency v1. It should prove context atlas, pattern cards, wave preparation, parent-hotfix lineage, verification receipts, ready-queue planning, and batch verification agree on a single productive-only synthetic wave."

  :write-scope
    ["scripts/check-context-atlas.mjs"
     "scripts/check-pattern-card.mjs"
     "scripts/prepare-task-runner-wave.mjs"
     "scripts/check-task-report.mjs"
     "scripts/check-verification-receipt.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/wave29-*.lisp"
     ".missiond/tasks/wave29/manifest.lisp"
     ".missiond/tasks/wave29/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/verify-task-run.mjs"]

  :requirements
    ["Use one synthetic productive-only wave that includes context_atlas_path, pattern_card_path, parent-hotfix lineage, verification receipts, local/smoke tiers, heartbeat metadata, and a DAG where ready-queue scheduling saves time versus group barrier."
     "Pin layer-local failures near their owners: atlas checker, pattern checker, prep CLI, report checker, receipt checker, planner, renderer, and batch verifier should each have at least one wave29-07 fixture or assertion."
     "Prove shared preamble usage is auditable: generated trace/skeleton guidance includes a preamble-read event for trace-writable tasks."
     "Prove parent-hotfix lineage: final commit hash is authoritative, agent commit hash is preserved, and parent_patches files are repo-relative."
     "Prove receipt reuse is conservative: wrong commit/tier/command does not count as reusable evidence."
     "Prove no cargo is required for this Node/Lisp-only smoke. Do not touch crates/**."]

  :acceptance
    ["node scripts/check-context-atlas.mjs --dry-fixture"
     "node scripts/check-pattern-card.mjs --dry-fixture"
     "node scripts/prepare-task-runner-wave.mjs --dry-fixture"
     "node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/check-verification-receipt.mjs --dry-fixture"
     "node scripts/plan-task-runner.mjs --dry-fixture"
     "node scripts/render-wave-briefs.mjs --dry-fixture"
     "node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/check-context-atlas.mjs scripts/check-pattern-card.mjs scripts/prepare-task-runner-wave.mjs scripts/check-task-report.mjs scripts/check-verification-receipt.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs scripts/verify-task-runner-batch.mjs"]

  :commit
    (:required true
     :message "test(tasks): smoke runner efficiency loop"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Smoke layers pinned."
     "Ready-queue savings and receipt reuse proofs."
     "Acceptance command results."])
