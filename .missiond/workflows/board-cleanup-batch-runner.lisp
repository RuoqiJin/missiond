;; MissionD workflow: Board cleanup batch runner.
;;
;; Purpose:
;;   Turn historical BoardTask cleanup from ad hoc manual review into a
;;   checkpointed, artifact-first workflow. Review workers may investigate and
;;   recommend, but they must not mutate historical BoardTasks directly.

(workflow board-cleanup-batch-runner
  :schema "missiond.workflow.v1"
  :workflow_id board-cleanup-batch-runner
  :status active
  :source_plans [execution-control-plane board-cleanup-observation memory-review-batch-runner]
  :match_rules
    ((trigger :kind BoardTask :title-prefix "Review MissionD related BoardTasks")
     (trigger :kind workflow-run :workflow board-cleanup-batch-runner)
     (dedupe-key "board-cleanup:<project>:<batch-id>"))
  :inputs [BoardTask-query project-id task-id-list review-scope context-pack-path?]
  :steps
    ((step s1 :id review-question :name review-question
       :entry [cleanup-objective project-id]
       :logic "Ask what must be known before closing historical tasks: whether each task is covered by SSOT, code, checker, deployment evidence, or still needs a fresh implementation task.")
     (step s2 :id validate-candidate-ids :name validate-candidate-ids
       :entry [task-id-list BoardTask-query]
       :logic "Validate every candidate as a full UUID before dispatch. Short, padded, duplicate, or missing ids fail the batch before any worker is launched.")
     (step s3 :id materialize-batch-context :name materialize-batch-context
       :entry [valid-task-ids project-id]
       :logic "Write a context-pack with task title, description, status, notes, candidate evidence paths, and allowed classification labels; do not preload broad KB/history.")
     (step s4 :id dispatch-fact-check-workers :name dispatch-fact-check-workers
       :entry [context-pack read-scope]
       :logic "Dispatch read-only investigator workers with heuristic questions and fixed output headings. Workers may read SSOT, code anchors, checker output, and existing Board notes; they may not close or edit historical tasks.")
     (step s5 :id collect-task-result-artifacts :name collect-task-result-artifacts
       :entry [worker-final provider-durable-log report-file]
       :logic "Normalize each worker result into task-result-artifact. Provider session final, PTY final, and Board note are projections; the report artifact under .missiond/research/board-cleanup is the canonical review result.")
     (step s6 :id classify-each-task :name classify-each-task
       :entry [task-result-artifact candidate-task]
       :logic "Classify each historical task as covered-by-ssot, covered-by-code, duplicate, obsolete, needs-new-task, needs-human, or keep-open. Use phase-comment anchors such as // Phase 6.x, duplicate title/project/day clusters, and source ownership migration evidence when available.")
     (step s7 :id synthesize-batch-report :name synthesize-batch-report
       :entry [classifications conflicts]
       :logic "Write a batch report with Findings, Evidence, Recommendations, Verification, per-task decision rationale, and any proposed new task descriptions.")
     (step s8 :id close-generated-review-task :name close-generated-review-task
       :entry [batch-report generated-review-task]
       :logic "Only the generated review BoardTask may be closed by this workflow after durable artifact settle. Historical tasks remain untouched unless a later operator or approved maintenance workflow applies the recommendations.")
     (step s9 :id update-cursors :name update-cursors
       :entry [batch-report workflow-run]
       :logic "Checkpoint reviewed ids, next cursor, generated report path, rejected ids, and unresolved needs-human items so the next batch can resume without re-reading completed items."))
  :egress [workflow_run task-result-artifact board-cleanup-batch-report cleanup-recommendation needs-human-report]
  :runtime-surfaces
    [.missiond/research/board-cleanup
     mission_shared_memory.task_result_put
     mission_board_query
     mission_board_note_add
     mission_board_update
     mission_task_delegate]
  :artifact-contract
    ((result :schema "missiond.board-cleanup-result.v1"
       :required [batch_id project_id reviewed_task_ids classifications evidence_refs recommendations verification]
       :headings [Findings Evidence Recommendations Verification]
       :allowed-classifications [covered-by-ssot covered-by-code duplicate obsolete needs-new-task needs-human keep-open])
     (classification :required [task_id classification rationale evidence_refs confidence]))
  :risk-gates
    ((gate g1 :rule "Historical BoardTasks are read-only by default; review workers never close, delete, hide, or rewrite them.")
     (gate g2 :rule "A generated review BoardTask may be closed only after a task-result-artifact exists and worker-completion-settle has bound taskId, slotId, ended_at, and report path.")
     (gate g3 :rule "A reused provider session is not completion authority; current task artifact and report path are required.")
     (gate g4 :rule "Output-contract close blocker accepts this workflow's Findings/Evidence/Recommendations/Verification report, not an intermediate assistant sentence.")
     (gate g5 :rule "Duplicate cleanup BoardTasks use semantic dedupe key board-cleanup:<project>:<batch-id>; repeated evidence appends to the same workflow_run.")
     (gate g6 :rule "If the review discovers an implementation gap, create a fresh exact-shard task proposal instead of repurposing a stale historical task."))
  :completion
    ((criterion c1 :rule "Every candidate id is either reviewed or rejected with invalid-id evidence before worker dispatch.")
     (criterion c2 :rule "Every completed worker has one task-result-artifact and one durable report path.")
     (criterion c3 :rule "The batch report includes per-task classification, evidence, recommendation, and verification.")
     (criterion c4 :rule "The generated review task is closed only after durable artifact settle.")
     (criterion c5 :rule "Historical tasks are left unchanged unless an explicit follow-up maintenance workflow is approved."))
  :safety
    ((rule :id no-historical-mutation :text "Board cleanup review is advisory by default; do not bulk-close historical tasks while triage is still in progress.")
     (rule :id artifact-first :text "Canonical completion is task-result-artifact; Board note and PTY are only projections.")
     (rule :id no-recursive-worker-control :text "Review workers must not create their own subtask trees; MissionD workflow-runner owns batching and delegation.")))
