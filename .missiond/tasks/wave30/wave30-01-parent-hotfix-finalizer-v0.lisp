;; Wave 30 task contract.

(task wave30-01-parent-hotfix-finalizer-v0
  :schema "missiond.task-contract.v1"
  :title "Parent hotfix finalizer v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave30-02-staged-source-hygiene-v0"
               "wave30-03-atomic-lifecycle-event-log-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "B"
  :estimated-minutes 50
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave30/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave30/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Close the Wave29-03 lineage drift class by making parent hotfix finalization orchestrator-owned. A worker may produce a draft report and worker commit; if the parent applies a post-worker hotfix, the runner must append/consume the parent patch fact and project a finalized report whose commit_hash, final_commit_hash, verified_commit_hash, and parent_patches agree."

  :write-scope
    ["scripts/task-runner-finalize-report.mjs"
     "scripts/task-runner-parent-hotfix.mjs"
     "scripts/check-task-report.mjs"
     "scripts/verify-task-runner-batch.mjs"
     ".missiond/tasks/schema/report-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/task-runner-manifest-v1.lisp"
     ".missiond/tasks/schema/task-runner-manifest-v2.lisp"
     ".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
     ".missiond/tasks/schema/verification-receipt-v1.lisp"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/wave30-*.lisp"
     ".missiond/tasks/wave30/manifest.lisp"
     ".missiond/tasks/wave30/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/task-runner-append-event.mjs"
     "scripts/check-task-lifecycle-events.mjs"
     "scripts/project-task-lifecycle-ledger.mjs"
     "scripts/check-staged-source-hygiene.mjs"
     "scripts/check-verification-receipt.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/check-task-runner-manifest.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/prepare-task-runner-wave.mjs"]

  :requirements
    ["Create task-runner-finalize-report.mjs with named exports and --dry-fixture. It should accept a worker draft report plus finalization facts and emit a deterministic finalized report object/string without mutating git."
     "Create task-runner-parent-hotfix.mjs with a dry-run/default read-only planning mode plus an explicit write mode if file mutation is needed; it must document that parent hotfix commits are appended as lineage facts, not worker commit amendments."
     "Update check-task-report.mjs/report-contract docs so parent_patches tail commit, final_commit_hash, verified_commit_hash, and commit_hash drift rules are explicit and fixture-pinned."
     "Update verify-task-runner-batch.mjs so finalized reports are the completion truth and worker draft hashes can still match lineage roles."
     "Dogfood the Wave29-03 drift shape as a fixture: worker commit d36de80, parent hotfix d842b1d, finalized report commit_hash d842b1d, agent_commit_hash d36de80."
     "No spawn, no LLM, no network. Any git inspection must be read-only and optional; default fixtures must run in temp dirs."]

  :acceptance
    ["node scripts/task-runner-finalize-report.mjs --dry-fixture"
     "node scripts/task-runner-parent-hotfix.mjs --dry-fixture"
     "node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "perl -ne 'exit 1 if /\\x00/' scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/verify-task-runner-batch.mjs"
     "git diff --check -- scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/verify-task-runner-batch.mjs .missiond/tasks/schema/report-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): finalize parent hotfix lineage"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Finalizer CLI input/output contract."
     "Parent hotfix helper behavior and explicit mutation boundary."
     "Wave29-03 drift fixture result."
     "Acceptance command results."])

