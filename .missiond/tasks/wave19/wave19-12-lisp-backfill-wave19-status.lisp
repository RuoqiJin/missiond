;; Wave 19 task contract.

(task wave19-12-lisp-backfill-wave19-status
  :schema "missiond.task-contract.v1"
  :title "Lisp backfill Wave 19 status"
  :kind lisp-only
  :status ready
  :owner "resident-lisp-architect"
  :depends-on ["wave19-02-task-contract-verifier-v1"
               "wave19-03-report-contract-v1"
               "wave19-04-shared-memory-ledger-v0"
               "wave19-05-renderer-dispatch-brief-v1"
               "wave19-06-plan-task-contract-emitter-v0"
               "wave19-07-workstation-task-contract-consumer-v0"
               "wave19-08-execution-task-contract-completion-v0"
               "wave19-09-cross-plan-distill-auto-chain-v1"
               "wave19-10-plan-dag-forward-compensate-ref-v0"
               "wave19-11-execution-opened-dispatch-metadata-v1"]
  :dispatch-strategy "resident-lisp"
  :goal "Backfill MissionD v2 architecture Lisp after Wave 19 code tasks, preserving source-index section ids and shard boundaries."

  :write-scope
    [".missiond/v2/intent-machine-contract.lisp"
     ".missiond/v2/intent-pillar-source-index.lisp"
     ".missiond/v2/intent-flow.lisp"
     ".missiond/v2/intent-intent-layer.lisp"
     ".missiond/v2/intent-tools.lisp"
     ".missiond/v2/intent-plan-dag.lisp"
     ".missiond/v2/intent-workstation-policy.lisp"
     ".missiond/v2/intent.lisp"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/tasks/**"
     ".missiond/claudecode/**"]

  :requirements
    ["Use the resident Lisp architect session if available."
     "Backfill only facts proven by committed Wave 19 tasks; do not mark pending items code-aligned early."
     "Preserve all section ids covered by R008/R015/R016/R017/R018."
     "Update source-index entries for any new machine-contract, task verifier, report, shared-memory, plan emitter, workstation consumer, and runtime pending closures."
     "Do not run broad Rust refactors."]

  :acceptance
    ["node scripts/check-architecture-lisp.mjs --all-v2"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-plan-dag.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent.lisp"]

  :commit
    (:required true
     :message "docs(v2): backfill wave19 machine-contract status"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Files updated."
     "Status changes by anchor."
     "Acceptance command results."])
