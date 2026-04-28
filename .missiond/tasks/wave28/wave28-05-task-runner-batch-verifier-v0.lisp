;; Wave 28 task contract.

(task wave28-05-task-runner-batch-verifier-v0
  :schema "missiond.task-contract.v1"
  :title "Task runner batch verifier v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave28-01-task-runner-manifest-schema-v0"
               "wave28-02-task-runner-plan-cli-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "C"
  :estimated-minutes 35
  :heartbeat-minutes 10
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Add a batch verifier that checks a task-runner manifest after worker commits: every productive node has a report, shared-memory completion, commit hash, and contract verification result. This prepares MissionD to close a full wave without parsing prose."

  :write-scope
    ["scripts/verify-task-runner-batch.mjs"
     "scripts/verify-task-run.mjs"]

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
     "scripts/plan-task-runner.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/render-claudecode-task.mjs"]

  :requirements
    ["CLI: node scripts/verify-task-runner-batch.mjs --manifest <manifest.lisp> [--json] [--dry-fixture]."
     "For each productive node, locate task contract, expected report, shared memory completion, and commit hash; reuse existing verify-task-run/checker helpers where possible."
     "Do not require archive/backfill/index pseudo-nodes. They are orchestrator-owned and outside worker completion accounting."
     "Output must include schema, manifest_path, wave, total_nodes, verified_nodes, missing_reports, missing_memory_completions, failed_contract_verifications, and aggregate_status."
     "Read-only only: no git mutation, no shell beyond existing read-only git helpers if imported from verifier, no network, no LLM."
     "Fixtures must cover all-green, missing report, missing memory completion, commit mismatch, and non-productive pseudo-node skipped."]

  :acceptance
    ["node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "node scripts/verify-task-run.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/verify-task-runner-batch.mjs scripts/verify-task-run.mjs"]

  :commit
    (:required true
     :message "feat(tasks): verify task runner batches"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Verifier output schema."
     "Skipped pseudo-node behavior."
     "Acceptance command results."])
