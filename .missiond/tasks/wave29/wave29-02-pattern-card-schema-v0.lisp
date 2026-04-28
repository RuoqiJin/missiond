;; Wave 29 task contract.

(task wave29-02-pattern-card-schema-v0
  :schema "missiond.task-contract.v1"
  :title "Pattern card schema v0"
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
  :context-atlas-path ".missiond/tasks/wave29/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave29/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Introduce a durable pattern-card schema/checker and seed reusable cards for schema checkers, read-only Node CLIs, report lineage, cross-layer smoke, and large-file navigation. Pattern cards are implementation recipes, not extra task scope."

  :write-scope
    [".missiond/tasks/schema/pattern-card-v1.lisp"
     "scripts/check-pattern-card.mjs"
     ".missiond/patterns/schema-checker.pattern.lisp"
     ".missiond/patterns/node-cli-readonly.pattern.lisp"
     ".missiond/patterns/report-lineage.pattern.lisp"
     ".missiond/patterns/cross-layer-smoke.pattern.lisp"
     ".missiond/patterns/large-file-navigation.pattern.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/wave29-*.lisp"
     ".missiond/tasks/wave29/manifest.lisp"
     ".missiond/tasks/wave29/dispatch-plan.lisp"
     ".missiond/tasks/wave29/context-atlas.lisp"
     ".missiond/claudecode/**"
     "scripts/check-context-atlas.mjs"
     "scripts/prepare-task-runner-wave.mjs"
     "scripts/check-task-report.mjs"
     "scripts/verify-task-run.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "scripts/plan-task-runner.mjs"]

  :requirements
    ["Define schema missiond.pattern-card.v1 in .missiond/tasks/schema/pattern-card-v1.lisp."
     "Checker must validate card id, schema, use_for task ids, recipe vectors, known_good paths, and optional anti-pattern notes."
     "Create five seed pattern cards under .missiond/patterns: schema-checker, node-cli-readonly, report-lineage, cross-layer-smoke, large-file-navigation."
     "Pattern cards must stay repo-relative and read-only guidance. They must not grant scope beyond the task contract."
     "Checker must support --json, --stdin, and --dry-fixture and reuse scripts/lib/missiond_lisp.mjs."
     "Fixtures must include valid multi-card input, duplicate card id rejection, empty recipe rejection, bad path rejection, and invalid use_for id rejection."]

  :acceptance
    ["node scripts/check-pattern-card.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/pattern-card-v1.lisp scripts/check-pattern-card.mjs .missiond/patterns/schema-checker.pattern.lisp .missiond/patterns/node-cli-readonly.pattern.lisp .missiond/patterns/report-lineage.pattern.lisp .missiond/patterns/cross-layer-smoke.pattern.lisp .missiond/patterns/large-file-navigation.pattern.lisp"]

  :commit
    (:required true
     :message "feat(tasks): add pattern card schema"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Seed cards created."
     "Fixture categories."
     "Acceptance command results."])
