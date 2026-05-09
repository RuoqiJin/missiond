(workflow memory-review-batch-runner
  :schema "missiond.workflow.v1"
  :workflow_id memory-review-batch-runner
  :status active
  :source_plans [conversation-memory-distillation execution-control-plane memory-kb-policy]
  :match_rules
    ((trigger :kind manual :tool mission_swarm_run :when "objective asks to review, shrink, archive, or adjudicate memory/Kb batches")
     (trigger :kind boardtask :title-prefix "KB memory triage wave")
     (dedupe-key "memory-review-batch-runner:<parent_task_id>:<manifest_hash>"))
  :owner missiond
  :purpose "Run long memory review waves as checkpointed MissionD workflow runs instead of ad-hoc launchd supervisors or prompt-only coordination."
  :inputs [memory-review-manifest true-user-utterance-export active-ssot-scope review-rubric worker-report-dir knowledge_review_state]
  :entry [BoardTask workflow.lisp batch-manifest memory-review-supervisor]
  :steps
    ((step s1 :id load-checkpoint
       :logic "Load workflow_run checkpoint with batch cursor, parent BoardTask, max_inflight, active worker task ids, collected artifact ids, and retry budget.")
     (step s2 :id dispatch-batch
       :logic "Dispatch only bounded reviewer batches to ClaudeCode read-only workers; each worker receives batch path, read_scope, no-write rule, and structured report contract.")
     (step s3 :id observe-eventbus
       :logic "Observe BoardTask, Slot, ConversationFinalDurable, and task-result-artifact events; polling is fallback only and must write a diagnostic. blocked/skipped child tasks are terminal diagnostics for batch capacity, not permanent inflight work.")
     (step s4 :id reap-and-retry
       :logic "Reap stale worker tasks by durable event/lease state, retry within budget, and never create duplicate worker tasks for a completed batch.")
     (step s5 :id collect-results
       :logic "Normalize worker final/report/Board note into task-result-artifact records; Board notes are projection only.")
     (step s6 :id codex-adjudication
       :logic "Codex or resident master adjudicates worker findings into active, superseded-by-lisp, superseded-by-code, historical-evidence, duplicate, wrong-or-stale, delete-candidate, or needs-human overlay decisions.")
     (step s7 :id write-review-overlay
       :logic "Write knowledge_review_state overlay after adjudication; do not mutate original knowledge rows or physically delete entries in the batch runner.")
     (step s8 :id final-collection
       :logic "Write final collection artifact with counts, unresolved needs-human items, skipped batches, and next resume cursor."))
  :egress [workflow_run task-result-artifact memory-review-final-collection knowledge_review_state_overlay needs-human-report]
  :risk-gates
    ((gate g1 :rule "Do not run if free disk is below the configured workflow-runner minimum; pause and write diagnostic instead.")
     (gate g2 :rule "Review workers are read-only and may write only their declared report artifact.")
     (gate g3 :rule "The batch runner never physically deletes knowledge rows; delete-candidate requires a separate deletion window and manifest.")
     (gate g4 :rule "Default review context excludes KB prefetch noise, cold runtime logs, and provider durable logs unless the manifest names them explicitly.")
     (gate g5 :rule "Each completed worker must produce a task-result-artifact before the parent batch can advance."))
  :completion
    ((criterion c1 :rule "Every manifest batch is done, skipped with rationale, or waiting on needs-human adjudication.")
     (criterion c2 :rule "Every completed worker has a task-result-artifact and ended_at conversation evidence.")
     (criterion c3 :rule "knowledge_review_state overlay count matches the final collection report.")
     (criterion c4 :rule "A resume cursor exists even when the workflow finishes.")))
