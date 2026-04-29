;; Wave 52 context-pack: already-integrated single-shard plan.

(context-pack wave52-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave52
  :purpose "Close the wave51 verifier gap: task contract verification must validate Lisp artifacts touched by the commit being verified."
  :write-model append-only
  :sequence 3

  (observation :id wave52-obs-wave51-artifact-gap
    :agent codex-parent
    :seq 1
    :at "2026-04-29T09:12:00Z"
    :summary "Wave51 worker commit 7f462d17 passed node scripts/verify-task-contract.mjs --commit=7f462d17 even though the commit carried .missiond/tasks/wave51/session-trace.lisp with (trace-event :kind acceptance), which check-session-trace rejects. Parent had to patch the ledger later. This shows verify-task-contract is still commit/scope-only and not yet a Lisp artifact validator."
    :files ["scripts/verify-task-contract.mjs"
            "scripts/check-session-trace.mjs"
            ".missiond/tasks/wave51/session-trace.lisp"
            ".missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp"])

  (shard-proposal :id wave52-shard-contract-artifact-validation
    :agent codex-parent
    :seq 2
    :at "2026-04-29T09:12:05Z"
    :summary "Implementation shard: keep verifyContract(contract, commitInfo) pure for importers, but make the verify-task-contract CLI discover known Lisp artifacts touched by the resolved commit, materialize those exact commit bytes into a temporary validation tree, and run existing checkers against them. Pin session-trace, shared-memory, task-lifecycle-events/event files, and report artifacts. Add fixture coverage and a live negative regression using wave51 worker commit 7f462d17 so invalid :kind acceptance cannot pass again."
    :shard contract-artifact-validation
    :owner claudecode
    :write-scope ["scripts/verify-task-contract.mjs"
                  ".missiond/v3/missiond-blueprint.lisp"
                  "scripts/check-v3-task-lifecycle-isomorphism.mjs"]
    :must-not-touch ["packages/**"
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
                     ".missiond/claudecode/**"]
    :acceptance ["node scripts/verify-task-contract.mjs --dry-fixture"
                 "node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture"
                 "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
                 "node scripts/check-v3-code-isomorphism-complete.mjs"
                 "if node scripts/verify-task-contract.mjs --commit=7f462d17 .missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp >/tmp/wave52-invalid-trace.out 2>&1; then cat /tmp/wave52-invalid-trace.out; exit 1; else rg \"session-trace|acceptance|artifact\" /tmp/wave52-invalid-trace.out; fi"
                 "git diff --check -- scripts/verify-task-contract.mjs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-task-lifecycle-isomorphism.mjs"])

  (integration-plan :id wave52-integration-plan-001
    :agent codex-integrator
    :seq 3
    :at "2026-04-29T09:12:10Z"
    :summary "Accepted one implementation shard. This wave is intentionally narrow: it turns a parent-observed wave51 ledger repair into a deterministic task-runner verifier invariant."
    :accepted-shards [contract-artifact-validation]
    :dispatch-groups [(group :id A :shards [contract-artifact-validation])])
)
