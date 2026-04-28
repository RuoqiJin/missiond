;; Wave 29 task contract.

(task wave29-04-parent-hotfix-lineage-v1
  :schema "missiond.task-contract.v1"
  :title "Parent hotfix lineage v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 30
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave29/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave29/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Harden the parent-hotfix commit lineage model introduced during wave29 prep. Parent one-line fixes should update the final report hash and record :agent_commit_hash plus :parent_patches, without amending worker commits and without breaking batch verification."

  :write-scope
    ["scripts/check-task-report.mjs"
     "scripts/verify-task-run.mjs"
     "scripts/verify-task-runner-batch.mjs"
     ".missiond/tasks/schema/report-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/task-runner-manifest-v1.lisp"
     ".missiond/tasks/schema/context-atlas-v1.lisp"
     ".missiond/tasks/schema/pattern-card-v1.lisp"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/wave29-*.lisp"
     ".missiond/tasks/wave29/manifest.lisp"
     ".missiond/tasks/wave29/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/check-context-atlas.mjs"
     "scripts/check-pattern-card.mjs"
     "scripts/check-verification-receipt.mjs"
     "scripts/prepare-task-runner-wave.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/plan-task-runner.mjs"]

  :requirements
    ["Model the wave28-02 case explicitly in fixtures: worker commit 954116e followed by parent lint-cleanup commit 302330a, final report :commit_hash equal to final commit, and :agent_commit_hash equal to worker commit."
     "Report checker must reject parent patches with missing commit/kind/reason/files, absolute/traversal files, malformed hashes, and final/verified hash drift."
     "verify-task-run must expose lineage in JSON and verify against final/verified commit when provided, while preserving existing reports without lineage fields."
     "verify-task-runner-batch must accept memory completion summaries that mention either the final commit or the agent commit, but the verified result should point at the final/verified hash."
     "No git mutation. Verifier commands remain read-only."
     "Add fixtures without reducing existing wave23/wave28 fixture coverage."]

  :acceptance
    ["node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/verify-task-run.mjs --dry-fixture"
     "node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "node scripts/check-task-report.mjs --all"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/check-task-report.mjs scripts/verify-task-run.mjs scripts/verify-task-runner-batch.mjs .missiond/tasks/schema/report-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): verify parent hotfix lineage"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Lineage fields and verifier behavior."
     "Wave28-02 hotfix fixture behavior."
     "Acceptance command results."])
