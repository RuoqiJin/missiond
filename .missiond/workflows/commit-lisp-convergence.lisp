;; MissionD workflow: commit -> Lisp/checker/evidence convergence.
;;
;; Purpose: once an AI worker has produced a commit, verify that the committed
;; behavior is represented in Lisp SSOT or create a visible backfill task.

(workflow commit-lisp-convergence
  :schema "missiond.workflow.v1"
  :workflow_id commit-lisp-convergence
  :status active
  :source_plans [lisp-code-drift-policy resident-master-control]
  :match_rules
    ((trigger :kind event :event SystemEvent.ContextualCommitDetected)
     (trigger :kind evidence :source mission_execution.complete :field commit_hash)
     (dedupe-key "commit-lisp-backfill:<project>:<sha>"))
  :inputs [commit-hash provider-log mission-execution-completion project-registry v3-blueprint project-blueprint-registry]
  :steps
    ((step s1 :name resolve-project
       :logic "Resolve project from commit event project_id, slot cwd/project_root, or project registry longest-prefix match.")
     (step s2 :name classify-unavailable-commit
       :logic "If the commit hash cannot be found in the slot root, conversation project root, or any registered local project root, classify it as external-or-unavailable-commit and write a diagnostic report only; do not imply a registry defect with unknown-project and do not create a backfill task.")
     (step s3 :name read-commit-snapshot
       :logic "Use git diff-tree --root --no-commit-id -r --name-only <sha> in the project root; never use current worktree diff as commit truth.")
     (step s4 :name classify-files
       :logic "Classify changed files as code, Lisp, checker, evidence, docs, or unknown.")
     (step s5 :name decide-coverage
       :logic "covered when code changes have same-commit Lisp/checker/evidence coverage; lisp-only/checker-only commits do not recurse; code-only commits need backfill.")
     (step s6 :name create-backfill-task
       :logic "For needs-backfill, create one visible BoardTask with commit hash, changed code files, provider conversation, suspected surfaces, and exact acceptance.")
     (step s7 :name optional-auto-delegate
       :logic "When runtime policy enables safe-backfill, delegate the backfill to ClaudeCode Opus via BoardTask/Autopilot; otherwise leave the task open.")
     (step s8 :name report
       :logic "Write .missiond/v3/runtime/commit-lisp-convergence/<sha>.report.lisp with status, task id, and evidence refs."))
  :egress [commit-convergence-report backfill-boardtask commit-convergence-status]
  :risk-gates
    ((gate g1 :rule "Commit coverage is computed from committed snapshots, not dirty worktree state.")
     (gate g2 :rule "Backfill tasks are visible, deduped, and never hide/delete historical Board tasks.")
     (gate g3 :rule "Lisp/checker/evidence-only backfill commits do not recursively create another backfill task.")
     (gate g4 :rule "Routine implementation goes through BoardTask/Autopilot, not legacy direct slot execution."))
  :completion
    ((criterion c1 :rule "Each commit event is classified as covered, needs-backfill, lisp-only, checker-only, no-changed-files, external-or-unavailable-commit, or unknown-project.")
     (criterion c2 :rule "Needs-backfill creates exactly one visible deduped BoardTask with commit hash and changed code evidence.")
     (criterion c3 :rule "A report is written under .missiond/v3/runtime/commit-lisp-convergence/<sha>.report.lisp."))
  :guardrails
    ((rule :id no-hidden-work :text "Backfill tasks are visible and deduped; old Board tasks are not hidden or deleted.")
     (rule :id no-recursion :text "Lisp/checker/evidence-only backfill commits must not create another backfill task.")
     (rule :id snapshot-truth :text "Commit coverage is computed from the committed snapshot, not from HEAD worktree state.")
     (rule :id boardtask-first :text "Routine implementation goes through BoardTask/Autopilot, not legacy direct slot_manager.execute.")))
