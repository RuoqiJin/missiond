;; Wave 22 task contract.

(task wave22-08-lisp-backfill-wave22-status
  :schema "missiond.task-contract.v1"
  :title "Lisp backfill Wave 22 status"
  :kind lisp-only
  :status ready
  :owner "resident-lisp-architect"
  :depends-on ["wave22-01-hooks-default-on-doctor-v2"
               "wave22-02-execution-auto-run-verifier-v2"
               "wave22-03-review-llm-approve-apply-gate-v1"
               "wave22-04-persisted-plan-inference-apply-v2"
               "wave22-05-autonomous-workstation-true-spawn-v1"
               "wave22-06-distill-chain-policy-auto-sonnet-v2"
               "wave22-07-autonomous-loop-apply-smoke-v4"]
  :dispatch-strategy "resident-lisp"
  :goal "Backfill MissionD v2 Lisp for Wave 22, marking explicit apply gates, auto verification, autonomous spawn gate, and remaining pending boundaries accurately."

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
     "Backfill only committed facts; any skipped/no-op task must stay marked pending or no-op."
     "Distinguish policy-gated automatic action from unconditional autonomy."
     "Preserve source-index and shard checker invariants."
     "Keep frontend Lisp explicitly postponed."]

  :acceptance
    ["node scripts/check-architecture-lisp.mjs --all-v2"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-plan-dag.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp"]

  :commit
    (:required true
     :message "docs(v2): backfill wave22 apply-gate status"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Anchors updated."
     "Automatic vs policy-gated distinction."
     "Remaining pending list."
     "Acceptance command results."])
