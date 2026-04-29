(session-trace wave52
  :schema "missiond.session-trace.v1"
  :wave wave52
  :created-at "2026-04-29T09:12:00Z"
  :sequence 0

  (trace-event
    :id wave52-trace-bootstrap-start-001
    :seq 1
    :at "2026-04-29T09:13:38Z"
    :task wave52-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave52-trace-bootstrap-read-001
    :seq 2
    :at "2026-04-29T09:13:38Z"
    :task wave52-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave52-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave52-01-trace-preamble-read-001
    :seq 3
    :at "2026-04-29T09:21:26Z"
    :task wave52-01-contract-artifact-validation-v0
    :backend claudecode
    :kind read
    :files [".missiond/claudecode/wave52-shared-preamble.md"
            ".missiond/tasks/wave52/wave52-01-contract-artifact-validation-v0.lisp"
            ".missiond/tasks/wave52/context-atlas.lisp"
            ".missiond/tasks/wave52/pattern-cards.lisp"
            ".missiond/tasks/wave52/context-pack.lisp"]
    :summary "Loaded shared preamble + task contract + atlas + pattern cards + context-pack integration plan before broad scans, per the audit expectation pinned by the wave52 bootstrap.")

  (trace-event
    :id wave52-01-trace-edit-001
    :seq 4
    :at "2026-04-29T09:25:00Z"
    :task wave52-01-contract-artifact-validation-v0
    :backend claudecode
    :kind edit
    :files ["scripts/verify-task-contract.mjs"
            ".missiond/v3/missiond-blueprint.lisp"
            "scripts/check-v3-task-lifecycle-isomorphism.mjs"]
    :summary "Added ARTIFACT_RULES + planArtifactValidation + validateCommitArtifacts to verify-task-contract; pinned the artifact-validator invariant in V3 blueprint surface task-runner-cli; added DEFAULT_FILES.contractVerifier + matching requireAll/dry-fixture in check-v3-task-lifecycle-isomorphism.")

  (trace-event
    :id wave52-01-trace-test-001
    :seq 5
    :at "2026-04-29T09:28:00Z"
    :task wave52-01-contract-artifact-validation-v0
    :backend claudecode
    :kind test
    :files ["scripts/verify-task-contract.mjs"
            "scripts/check-v3-task-lifecycle-isomorphism.mjs"]
    :command "node scripts/verify-task-contract.mjs --dry-fixture"
    :exit_code 0
    :summary "Dry fixtures green: 10 verifyContract fixtures + 6 helper cases + 3 artifact-plan cases + 1 artifact-checker case all pass.")

  (trace-event
    :id wave52-01-trace-acceptance-regression-001
    :seq 6
    :at "2026-04-29T09:29:00Z"
    :task wave52-01-contract-artifact-validation-v0
    :backend claudecode
    :kind failure
    :files [".missiond/tasks/wave51/session-trace.lisp"
            "scripts/check-session-trace.mjs"
            "scripts/verify-task-contract.mjs"]
    :commit_hash "7f462d17"
    :summary "Verified wave51 worker commit 7f462d17 now FAILS through the new artifact validator on session-trace.lisp's invalid :kind acceptance, exactly as the wave52 integration plan required.")

  (trace-event
    :id wave52-01-trace-complete-001
    :seq 7
    :at "2026-04-29T09:30:10Z"
    :task wave52-01-contract-artifact-validation-v0
    :backend claudecode
    :kind complete
    :files [".missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp"]
    :report_path ".missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp"
    :summary "Wave52-01 complete: report written, completion entry appended; ready for write-scope-only commit."))
