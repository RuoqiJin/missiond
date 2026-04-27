;; Wave 21 task contract.

(task wave21-09-lisp-backfill-wave21-status
  :schema "missiond.task-contract.v1"
  :title "Lisp backfill Wave 21 status"
  :kind lisp-only
  :status ready
  :owner "resident-lisp-architect"
  :depends-on ["wave21-01-hooks-path-installer-v1"
               "wave21-02-run-verifier-v1"
               "wave21-03-execution-report-verifier-integration-v1"
               "wave21-04-autonomous-workstation-llm-proposal-v0"
               "wave21-05-plan-inference-apply-gate-v1"
               "wave21-06-llm-auto-approve-proposal-v0"
               "wave21-07-sonnet-distill-chain-auto-apply-v1"
               "wave21-08-machine-contract-autonomous-loop-smoke-v3"]
  :dispatch-strategy "resident-lisp"
  :goal "Backfill MissionD v2 Lisp for Wave 21, marking default-on hook guardrails, task-run verification, and LLM proposal/apply gates accurately."

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
     "Backfill only committed facts; proposal-only tasks must not be marked as automatic execution."
     "Preserve all source-index and shard checker invariants."
     "Keep frontend Lisp explicitly postponed unless the user starts a frontend wave."]

  :acceptance
    ["node scripts/check-architecture-lisp.mjs --all-v2"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-plan-dag.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp"]

  :commit
    (:required true
     :message "docs(v2): backfill wave21 autonomous-loop status"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Anchors updated."
     "Proposal-only vs applied status distinctions."
     "Remaining pending list."
     "Acceptance command results."])
