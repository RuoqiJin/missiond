;; Wave 30 dispatch-time context atlas.
;; Read-only guidance for workers. Task contracts remain the source of truth.

(context-atlas wave30-lifecycle-finalization
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave30
  :goal "Turn Wave29 runner-efficiency mechanisms into an orchestrator-owned lifecycle protocol that advances the Lisp-driven MissionD execution loop."
  :read-order [".missiond/claudecode/wave30-shared-preamble.md"
               ".missiond/tasks/wave30/context-atlas.lisp"
               ".missiond/tasks/wave30/pattern-cards.lisp"
               ".missiond/research/result.md"
               ".missiond/research/wave30-codex-action-plan.md"
               ".missiond/tasks/wave30/<task-id>.lisp"]

  (global-anchors
    (file "scripts/check-task-report.mjs"
      :purpose "Report checker and commit lineage validator."
      :grep ["validateCommitLineage"
             ":agent_commit_hash"
             ":parent_patches"
             "COMMIT_HASH_REGEX"])
    (file "scripts/verify-task-run.mjs"
      :purpose "Single task/report/memory/commit verifier; exposes lineage roles."
      :grep ["agentCommitHash"
             "finalCommitHash"
             "verifiedCommitHash"
             "lineage_match"])
    (file "scripts/verify-task-runner-batch.mjs"
      :purpose "Manifest-wide verifier; Wave30 binds finalized reports and receipts here."
      :grep ["receipt_coverage"
             "lineage"
             "aggregateResults"
             "verifyManifest"])
    (file "scripts/check-verification-receipt.mjs"
      :purpose "Receipt schema/checker; reuse tier and commit/file coverage rules."
      :grep ["valid_for_files"
             "tier"
             "exit_code"
             "hex-prefix-agree"])
    (file "scripts/plan-task-runner.mjs"
      :purpose "Group-barrier and ready-queue planner."
      :grep ["computeReadyQueue"
             "barrier_finish_at"
             "ready_queue"
             "schedule"])
    (file "scripts/check-task-runner-manifest.mjs"
      :purpose "Manifest checker and shared manifest projection helpers."
      :grep ["validateManifestObject"
             "projectManifest"
             "overlap_policy"
             "depends_on"])
    (file "scripts/render-wave-briefs.mjs"
      :purpose "Thin brief batch renderer; Wave30 hard/soft references should surface here."
      :grep ["renderManifest"
             "renderTask"
             "shared_preamble_path"])
    (file "scripts/prepare-task-runner-wave.mjs"
      :purpose "Wave preparation CLI; currently emits bootstrap shared-memory/session-trace entries."
      :grep ["bootstrap"
             "preamble"
             "spliceBeforeFinalParen"
             "session-trace"])
    (file ".githooks/pre-commit"
      :purpose "Repo-local hook entrypoint; Wave30 may route staged source hygiene here."
      :grep ["MISSIOND_TASK_CONTRACT"
             "task-scope-guard"])
    (file ".missiond/v2/intent-machine-contract.lisp"
      :purpose "Architecture SSOT that defines Wave30 lifecycle finalization as mainline."
      :grep ["s10-finalize-lifecycle-truth"
             "task-lifecycle-event-lisp"
             "finalized-report-lisp"]))

  (task-focus
    (task wave30-01-parent-hotfix-finalizer-v0
      :first-reads ["scripts/check-task-report.mjs"
                    "scripts/verify-task-runner-batch.mjs"
                    ".missiond/tasks/wave29/reports/wave29-03-runner-wave-prep-v0.report.lisp"
                    ".missiond/research/result.md"]
      :avoid ["Do not ask workers to amend old commits. Model finalization as a new orchestrator-owned projection."])
    (task wave30-02-staged-source-hygiene-v0
      :first-reads [".githooks/pre-commit"
                    "scripts/check-missiond-hooks.mjs"
                    "scripts/install-missiond-hooks.mjs"
                    "scripts/task-scope-guard.mjs"]
      :avoid ["Do not make global hooks mandatory. Keep repo-local and explicit unless an existing contract already opts in."])
    (task wave30-03-atomic-lifecycle-event-log-v0
      :first-reads ["scripts/prepare-task-runner-wave.mjs"
                    ".missiond/tasks/schema/shared-memory-v1.lisp"
                    ".missiond/tasks/schema/session-trace-v1.lisp"
                    "scripts/check-task-memory.mjs"
                    "scripts/check-session-trace.mjs"]
      :avoid ["Do not let workers edit shared-memory/session-trace directly in new paths; add an append helper and projection layer."])
    (task wave30-04-manifest-hard-soft-deps-v2
      :first-reads ["scripts/plan-task-runner.mjs"
                    "scripts/check-task-runner-manifest.mjs"
                    "scripts/render-wave-briefs.mjs"
                    ".missiond/tasks/wave29/manifest.lisp"]
      :avoid ["Do not break v1 manifests. Add v2 or additive compatibility, and keep default output compatible."])
    (task wave30-05-lifecycle-receipt-smoke-v0
      :first-reads ["scripts/task-runner-finalize-report.mjs"
                    "scripts/task-runner-append-event.mjs"
                    "scripts/check-staged-source-hygiene.mjs"
                    "scripts/verify-task-runner-batch.mjs"]
      :avoid ["No cargo unless a previous Wave30 task touched Rust. This smoke is Node/Lisp orchestration."])))

