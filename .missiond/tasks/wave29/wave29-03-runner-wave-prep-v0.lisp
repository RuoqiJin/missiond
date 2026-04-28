;; Wave 29 task contract.

(task wave29-03-runner-wave-prep-v0
  :schema "missiond.task-contract.v1"
  :title "Runner wave prep v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave29-01-context-atlas-schema-v0"
               "wave29-02-pattern-card-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "B"
  :estimated-minutes 35
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave29/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave29/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Add a read-only-plus-file-generation preparation CLI for future task-runner waves: validate a manifest, render thin briefs, prepare report skeleton paths, and emit bootstrap shared-memory/session-trace entries including an auditable shared-preamble-read expectation. This reduces per-agent report/setup work without creating archive/backfill/index workers."

  :write-scope
    ["scripts/prepare-task-runner-wave.mjs"
     "scripts/render-wave-briefs.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/context-atlas-v1.lisp"
     ".missiond/tasks/schema/pattern-card-v1.lisp"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/wave29-*.lisp"
     ".missiond/tasks/wave29/manifest.lisp"
     ".missiond/tasks/wave29/dispatch-plan.lisp"
     ".missiond/tasks/wave29/context-atlas.lisp"
     ".missiond/tasks/wave29/pattern-cards.lisp"
     ".missiond/claudecode/**"
     ".missiond/patterns/**"
     "scripts/check-context-atlas.mjs"
     "scripts/check-pattern-card.mjs"
     "scripts/check-task-report.mjs"
     "scripts/verify-task-run.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "scripts/plan-task-runner.mjs"]

  :requirements
    ["CLI: node scripts/prepare-task-runner-wave.mjs --manifest <manifest.lisp> [--out-dir <repo>] [--dry-run] [--force] [--json] [--dry-fixture]."
     "Reuse render-wave-briefs internals through named exports instead of shelling out; if needed, export renderManifest from scripts/render-wave-briefs.mjs while preserving CLI behavior."
     "Prepare reports directory and optional report skeletons in a deterministic form, but do not stage or commit generated wave artifacts from fixtures."
     "Emit or print bootstrap shared-memory/session-trace entries that include a preamble-read trace expectation for trace-writable tasks."
     "Respect productive-only: archive/backfill/index/lisp-backfill pseudo-nodes remain orchestrator-owned and must not receive worker briefs or report skeletons."
     "Fixtures must use temporary repos only and cover dry-run no-write, force overwrite, report skeleton generation, preamble-read trace event, pseudo-node rejection, and deterministic output."]

  :acceptance
    ["node scripts/prepare-task-runner-wave.mjs --dry-fixture"
     "node scripts/render-wave-briefs.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/prepare-task-runner-wave.mjs scripts/render-wave-briefs.mjs"]

  :commit
    (:required true
     :message "feat(tasks): prepare task runner waves"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Prepared artifact types."
     "How preamble-read trace evidence is represented."
     "Acceptance command results."])
