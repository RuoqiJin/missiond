;; Wave 37 task-runner manifest.

(task-runner-manifest wave37-request-receipt-projection-v0
  :schema "missiond.task-runner-manifest.v1"
  :wave wave37
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave37-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave37-01-request-verification-receipt-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope ["scripts/check-verification-receipt.mjs"
                      "scripts/check-v3-task-lifecycle-isomorphism.mjs"
                      "scripts/verify-task-runner-batch.mjs"
                      ".missiond/v3/missiond-blueprint.lisp"]))
