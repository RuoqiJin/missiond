;; Wave 28 dispatch plan.
;; This is orchestration metadata, not a ClaudeCode worker task.

(dispatch-plan wave28
  :schema "missiond.dispatch-plan.v0"
  :policy "productive-only"
  :shared-preamble ".missiond/claudecode/wave28-shared-preamble.md"
  :brief-mode thin
  :archive-policy "orchestrator-owned; never a worker task"
  :lisp-backfill-policy "codex-owned after code tasks; never a worker task"
  :index-policy "dispatch-plan.lisp replaces parallel-dispatch-index worker task"

  :nodes
    [(node wave28-01-task-runner-manifest-schema-v0
       :group A
       :verification-tier local
       :estimated-minutes 30
       :write-scope [".missiond/tasks/schema/task-runner-manifest-v1.lisp"
                     "scripts/check-task-runner-manifest.mjs"])
     (node wave28-02-task-runner-plan-cli-v0
       :group B
       :depends-on [wave28-01-task-runner-manifest-schema-v0]
       :verification-tier local
       :estimated-minutes 35
       :write-scope ["scripts/plan-task-runner.mjs"])
     (node wave28-03-wave-brief-batch-renderer-v0
       :group B
       :depends-on [wave28-01-task-runner-manifest-schema-v0]
       :verification-tier local
       :estimated-minutes 25
       :write-scope ["scripts/render-wave-briefs.mjs"
                     "scripts/render-claudecode-task.mjs"])
     (node wave28-04-mission-plan-task-runner-dry-run-surface-v0
       :group C
       :depends-on [wave28-01-task-runner-manifest-schema-v0
                    wave28-02-task-runner-plan-cli-v0]
       :verification-tier smoke
       :estimated-minutes 60
       :write-scope ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"])
     (node wave28-05-task-runner-batch-verifier-v0
       :group C
       :depends-on [wave28-01-task-runner-manifest-schema-v0
                    wave28-02-task-runner-plan-cli-v0]
       :verification-tier local
       :estimated-minutes 35
       :write-scope ["scripts/verify-task-runner-batch.mjs"
                     "scripts/verify-task-run.mjs"])
     (node wave28-06-task-runner-loop-smoke-v0
       :group D
       :depends-on [wave28-02-task-runner-plan-cli-v0
                    wave28-03-wave-brief-batch-renderer-v0
                    wave28-04-mission-plan-task-runner-dry-run-surface-v0
                    wave28-05-task-runner-batch-verifier-v0]
       :verification-tier full
       :estimated-minutes 45
       :write-scope ["scripts/check-task-runner-manifest.mjs"
                     "scripts/plan-task-runner.mjs"
                     "scripts/render-wave-briefs.mjs"
                     "scripts/verify-task-runner-batch.mjs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"])])
