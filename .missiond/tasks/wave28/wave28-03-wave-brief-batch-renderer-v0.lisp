;; Wave 28 task contract.

(task wave28-03-wave-brief-batch-renderer-v0
  :schema "missiond.task-contract.v1"
  :title "Wave brief batch renderer v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave28-01-task-runner-manifest-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "B"
  :estimated-minutes 25
  :heartbeat-minutes 10
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Add a batch renderer that consumes a task-runner manifest and emits one shared preamble plus thin ClaudeCode briefs for productive worker tasks. This makes Wave28+ dispatch use the new thin-brief path without hand-rendering every task."

  :write-scope
    ["scripts/render-wave-briefs.mjs"
     "scripts/render-claudecode-task.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/task-runner-manifest-v1.lisp"
     ".missiond/tasks/wave27/**"
     ".missiond/tasks/wave28/wave28-*.lisp"
     ".missiond/tasks/wave28/dispatch-plan.lisp"
     ".missiond/claudecode/wave27-*.md"
     "scripts/check-task-runner-manifest.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :requirements
    ["CLI: node scripts/render-wave-briefs.mjs --manifest <manifest.lisp> [--force] [--dry-fixture]."
     "Generate .missiond/claudecode/<wave>-shared-preamble.md once, then render each productive node with render-claudecode-task.mjs --brief-mode thin --shared-preamble <path>."
     "Do not render archive/backfill/index pseudo-nodes as worker briefs; fail if the manifest tries to mark them as productive worker nodes."
     "Prefer importing renderer functions only if the existing renderer can expose them cleanly; otherwise invoke renderer logic in-process without shelling out."
     "Keep render-claudecode-task.mjs full-mode backward compatible; existing dry fixtures must remain green."
     "Fixtures must prove preamble exists, thin brief omits repeated Shared Memory / Report Contract / Session Trace / Router Policy sections, and output paths are deterministic."]

  :acceptance
    ["node scripts/render-wave-briefs.mjs --dry-fixture"
     "node scripts/render-claudecode-task.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/render-wave-briefs.mjs scripts/render-claudecode-task.mjs"]

  :commit
    (:required true
     :message "feat(tasks): render wave briefs from manifest"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Generated output conventions."
     "Backward-compat notes for full renderer."
     "Acceptance command results."])
