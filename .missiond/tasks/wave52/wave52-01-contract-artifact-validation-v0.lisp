;; Wave 52 task contract.

(task wave52-01-contract-artifact-validation-v0
  :schema "missiond.task-contract.v1"
  :title "validate touched Lisp artifacts during task contract verification"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 55
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave52/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave52/pattern-cards.lisp"
  :context-pack-path ".missiond/tasks/wave52/context-pack.lisp"
  :goal "Upgrade scripts/verify-task-contract.mjs so commit verification also validates known Lisp artifacts touched by the verified commit. The wave51 worker commit 7f462d17 must now fail because it contains an invalid session-trace :kind acceptance."

  :write-scope
    ["scripts/verify-task-contract.mjs"
     ".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-task-lifecycle-isomorphism.mjs"
     ".missiond/tasks/wave52/shared-memory.lisp"
     ".missiond/tasks/wave52/session-trace.lisp"
     ".missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp"]

  :must-not-touch
    ["packages/**"
     "crates/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave48/**"
     ".missiond/tasks/wave49/**"
     ".missiond/tasks/wave50/**"
     ".missiond/tasks/wave51/**"
     ".missiond/tasks/wave52/manifest.lisp"
     ".missiond/tasks/wave52/wave52-*.lisp"
     ".missiond/tasks/wave52/context-atlas.lisp"
     ".missiond/tasks/wave52/pattern-cards.lisp"
     ".missiond/tasks/wave52/context-pack.lisp"
     ".missiond/claudecode/**"
     "scripts/check-session-trace.mjs"
     "scripts/check-task-memory.mjs"
     "scripts/check-task-report.mjs"
     "scripts/check-task-lifecycle-events.mjs"
     "scripts/task-scope-guard.mjs"
     "scripts/verify-task-run.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :requirements
    ["Read the shared preamble, this task contract, context atlas, pattern cards, and the wave52 context-pack integration-plan before broad scans."
     "Use scripts/context-pack-compile-shards.mjs .missiond/tasks/wave52/context-pack.lisp to confirm this is the accepted mapped shard."
     "Extend scripts/verify-task-contract.mjs so real commit verification detects known Lisp artifacts touched by the resolved commit and validates them with the existing artifact checkers."
     "Known artifact paths: .missiond/tasks/<wave>/session-trace.lisp -> check-session-trace, shared-memory.lisp -> check-task-memory, task-lifecycle-events.lisp and events/*.event.lisp -> check-task-lifecycle-events, reports/*.report.lisp -> check-task-report."
     "Validate artifact bytes from the resolved commit, not the current working tree, so --commit=<worker-hash> remains correct after later parent commits."
     "Preserve the existing pure verifyContract(contract, commitInfo) API for importers; add artifact validation around the CLI path or through a clearly separated helper so verify-task-run and batch imports do not gain hidden disk side effects."
     "Add dry fixtures or focused regression guards proving artifact validation planning and the invalid session-trace case are covered."
     "Add a live regression command that expects node scripts/verify-task-contract.mjs --commit=7f462d17 .missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp to fail on the invalid session-trace artifact, not on commit message or scope."
     "Update .missiond/v3/missiond-blueprint.lisp and scripts/check-v3-task-lifecycle-isomorphism.mjs so this V3 task-runner-cli invariant is pinned."
     "Write the task report and commit only the declared write scope."]

  :acceptance
    ["node scripts/verify-task-contract.mjs --dry-fixture"
     "node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "node scripts/check-v3-code-isomorphism-complete.mjs"
     "if node scripts/verify-task-contract.mjs --commit=7f462d17 .missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp >/tmp/wave52-invalid-trace.out 2>&1; then cat /tmp/wave52-invalid-trace.out; exit 1; else rg \"session-trace|acceptance|artifact\" /tmp/wave52-invalid-trace.out; fi"
     "node scripts/check-task-report.mjs .missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp"
     "git diff --check -- scripts/verify-task-contract.mjs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-task-lifecycle-isomorphism.mjs .missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp"]

  :commit
    (:required true
     :message "fix(tasks): validate lisp artifacts during contract verify"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Artifact detection rules added to verify-task-contract."
     "How commit-specific artifact bytes are validated."
     "Evidence that wave51 commit 7f462d17 now fails for invalid session-trace."
     "Acceptance command results."])
