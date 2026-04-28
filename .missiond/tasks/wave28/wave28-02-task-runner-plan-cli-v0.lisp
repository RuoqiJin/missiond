;; Wave 28 task contract.

(task wave28-02-task-runner-plan-cli-v0
  :schema "missiond.task-contract.v1"
  :title "Task runner plan CLI v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave28-01-task-runner-manifest-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "B"
  :estimated-minutes 35
  :heartbeat-minutes 10
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Add a read-only CLI that consumes a task-runner manifest and task contracts, then emits a deterministic runner plan: topological groups, critical-path estimate, write-scope overlap diagnostics, and verification-tier summary. This is the first dry-run substrate for a future MissionD task-contract runner."

  :write-scope
    ["scripts/plan-task-runner.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/task-runner-manifest-v1.lisp"
     ".missiond/tasks/wave27/**"
     ".missiond/tasks/wave28/wave28-*.lisp"
     ".missiond/tasks/wave28/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/check-task-runner-manifest.mjs"
     "scripts/render-claudecode-task.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :requirements
    ["CLI: node scripts/plan-task-runner.mjs --manifest <manifest.lisp> [--json|--lisp] [--dry-fixture]."
     "Import the manifest checker or share a small parser helper from wave28-01; do not duplicate inconsistent validation rules."
     "Output must include schema, manifest_path, wave, productive_only, batches, critical_path_minutes, total_estimated_minutes, max_parallel_width, overlap_diagnostics, and verification_tier_counts."
     "Topological sort must be deterministic and fail on dependency cycles with a structured error."
     "No dispatch, no spawn, no git mutation, no network, no LLM. This CLI plans only."
     "Fixtures must cover valid manifest, cycle rejection, missing task contract rejection, overlap warning/error, and deterministic ordering."]

  :acceptance
    ["node scripts/plan-task-runner.mjs --dry-fixture"
     "node scripts/check-task-runner-manifest.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/plan-task-runner.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add task runner plan CLI"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Output schema fields."
     "Cycle/overlap behavior."
     "Acceptance command results."])
