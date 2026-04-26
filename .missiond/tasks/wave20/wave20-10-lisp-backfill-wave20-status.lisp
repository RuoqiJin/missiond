;; Wave 20 task contract.

(task wave20-10-lisp-backfill-wave20-status
  :schema "missiond.task-contract.v1"
  :title "Lisp backfill Wave 20 status"
  :kind lisp-only
  :status ready
  :owner "resident-lisp-architect"
  :depends-on ["wave20-01-task-scope-index-guard-v1"
               "wave20-02-renderer-scoped-commit-guard-v2"
               "wave20-03-execution-preflight-contract-scope-v1"
               "wave20-04-machine-driven-dispatch-v0"
               "wave20-05-unified-entry-machine-loop-smoke-v2"
               "wave20-06-cross-plan-distill-auto-trigger-v1"
               "wave20-07-llm-augmented-plan-inference-v0"
               "wave20-08-review-auto-answer-policy-v0"
               "wave20-09-execution-event-legacy-metadata-sweep"]
  :dispatch-strategy "resident-lisp"
  :goal "Backfill MissionD v2 architecture Lisp after Wave 20, with special focus on machine-contract dispatch, scoped-index guardrails, and remaining autonomous-loop boundaries."

  :write-scope
    [".missiond/v2/intent-machine-contract.lisp"
     ".missiond/v2/intent-pillar-source-index.lisp"
     ".missiond/v2/intent-flow.lisp"
     ".missiond/v2/intent-intent-layer.lisp"
     ".missiond/v2/intent-tools.lisp"
     ".missiond/v2/intent-plan-dag.lisp"
     ".missiond/v2/intent-workstation-policy.lisp"
     ".missiond/v2/intent-execution-governance.lisp"
     ".missiond/v2/intent.lisp"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/tasks/**"
     ".missiond/claudecode/**"]

  :requirements
    ["Use the resident Lisp architect session if available."
     "Backfill only committed facts; mark skipped/no-op tasks honestly."
     "Preserve all source-index and shard checker invariants."
     "Do not compress or split additional shards in this wave unless a checker demands it."
     "Keep frontend Lisp explicitly postponed."]

  :acceptance
    ["node scripts/check-architecture-lisp.mjs --all-v2"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-plan-dag.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp"]

  :commit
    (:required true
     :message "docs(v2): backfill wave20 machine-dispatch status"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Anchors updated."
     "Remaining pending list."
     "Acceptance command results."])
