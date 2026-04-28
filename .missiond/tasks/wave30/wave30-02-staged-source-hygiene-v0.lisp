;; Wave 30 task contract.

(task wave30-02-staged-source-hygiene-v0
  :schema "missiond.task-contract.v1"
  :title "Staged source hygiene v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 35
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave30/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave30/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Promote the Wave29 NUL-byte/diff-check lessons into a reusable staged source hygiene preflight that MissionD can run before final report projection and commit handoff."

  :write-scope
    ["scripts/check-staged-source-hygiene.mjs"
     "scripts/check-missiond-hooks.mjs"
     "scripts/install-missiond-hooks.mjs"
     ".githooks/pre-commit"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/**"
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
     "scripts/check-task-report.mjs"
     "scripts/check-verification-receipt.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/check-task-runner-manifest.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/prepare-task-runner-wave.mjs"]

  :requirements
    ["Create check-staged-source-hygiene.mjs with named exports and --dry-fixture. It should check staged or supplied files for raw NUL bytes, diff whitespace errors, and task-scope guard readiness."
     "Default operation must be read-only diagnostics. If hook integration is added, keep repo-local opt-in behavior and do not silently install global hooks."
     "Integrate the new checker into .githooks/pre-commit only behind existing MISSIOND_TASK_CONTRACT/repo-local guard semantics."
     "Update hook doctor output so it can report whether staged-source hygiene is available, without requiring git config mutation."
     "Fixture NUL detection using temp files or escaped byte writes inside the fixture; do not leave raw NUL bytes in repository source."]

  :acceptance
    ["node scripts/check-staged-source-hygiene.mjs --dry-fixture"
     "node scripts/check-missiond-hooks.mjs --dry-fixture"
     "node scripts/install-missiond-hooks.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "perl -ne 'exit 1 if /\\x00/' scripts/check-staged-source-hygiene.mjs scripts/check-missiond-hooks.mjs scripts/install-missiond-hooks.mjs .githooks/pre-commit"
     "git diff --check -- scripts/check-staged-source-hygiene.mjs scripts/check-missiond-hooks.mjs scripts/install-missiond-hooks.mjs .githooks/pre-commit"]

  :commit
    (:required true
     :message "feat(tasks): check staged source hygiene"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Source hygiene checks implemented."
     "Hook integration and mutation boundary."
     "NUL byte fixture result."
     "Acceptance command results."])

