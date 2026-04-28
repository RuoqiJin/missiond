;; Wave 30 task contract.

(task wave30-04-manifest-hard-soft-deps-v2
  :schema "missiond.task-contract.v1"
  :title "Manifest hard/soft deps v2"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave30-02-staged-source-hygiene-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "B"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave30/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave30/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Make ready-queue scheduling precise by separating hard dependencies that block dispatch from soft references that only enrich briefs/context. Preserve task-runner-manifest v1 compatibility while adding a v2/additive path for hard_deps, soft_refs, ready-queue release facts, and optional lifecycle lease/event-log references."

  :write-scope
    [".missiond/tasks/schema/task-runner-manifest-v2.lisp"
     "scripts/check-task-runner-manifest.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/render-wave-briefs.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
     ".missiond/tasks/schema/verification-receipt-v1.lisp"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/wave30-*.lisp"
     ".missiond/tasks/wave30/manifest.lisp"
     ".missiond/tasks/wave30/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/task-runner-finalize-report.mjs"
     "scripts/task-runner-parent-hotfix.mjs"
     "scripts/task-runner-append-event.mjs"
     "scripts/check-task-lifecycle-events.mjs"
     "scripts/project-task-lifecycle-ledger.mjs"
     "scripts/check-staged-source-hygiene.mjs"
     "scripts/check-task-report.mjs"
     "scripts/check-verification-receipt.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "scripts/prepare-task-runner-wave.mjs"]

  :requirements
    ["Add task-runner-manifest-v2.lisp or an explicitly additive v1-compatible schema note that distinguishes :hard_deps from :soft_refs. Existing :depends_on must keep v1 behavior."
     "Update check-task-runner-manifest.mjs to validate v2/additive hard/soft references without breaking all existing v1 fixtures."
     "Update plan-task-runner.mjs ready-queue mode so only hard dependencies block dispatch. Soft references may be reported as context but must not affect barrier_finish_at/ready time."
     "Update render-wave-briefs.mjs so soft references render as context guidance, not as dependencies or blockers."
     "Add a fixture matching the Wave29-03 observation: a task that only hard-depends on the manifest/atlas source must not wait for unrelated soft references."]

  :acceptance
    ["node scripts/check-task-runner-manifest.mjs --dry-fixture"
     "node scripts/plan-task-runner.mjs --dry-fixture"
     "node scripts/render-wave-briefs.mjs --dry-fixture"
     "node scripts/check-task-runner-manifest.mjs .missiond/tasks/wave30/manifest.lisp"
     "node scripts/check-task-contract.mjs --all"
     "perl -ne 'exit 1 if /\\x00/' scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs"
     "git diff --check -- .missiond/tasks/schema/task-runner-manifest-v2.lisp scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs"]

  :commit
    (:required true
     :message "feat(tasks): split hard and soft runner deps"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Manifest v2/additive compatibility strategy."
     "Ready-queue hard-vs-soft dependency fixture."
     "Renderer soft-reference output."
     "Acceptance command results."])
