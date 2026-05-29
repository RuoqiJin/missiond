(workflow work-order-lifecycle
  :schema "missiond.workflow.v1"
  :workflow_id work-order-lifecycle
  :status active
  :purpose "Unify Board-triggered, intent.lisp-triggered, and external-application-triggered worker delegation into one auditable chain."
  :source_plans [mission_request mission_board mission_workflow mission-shared-memory file-artifacts]
  :authority
    ((board "BoardTask is the coordination anchor and operator-facing state.")
     (intent "intent.lisp or equivalent external intent envelope is the normalized objective.")
     (plan "plan.lisp is the accepted forecast and decomposition source; it must compile into shard and atom contracts before workers execute.")
     (atom_graph "plan-atomization-graph records shard_nodes, atom_tasks, serial/parallel edges, and worker telemetry expectations.")
     (result "task-result-artifact is the canonical worker output.")
     (audit "audit.lisp is the replayable work-order history."))
  :entry
    ((source user_request :shape "natural-language request captured by resident master or mission_request")
     (source board_task :shape "BoardTaskCreated or mission_board_create")
     (source file_intent :shape ".missiond/work-orders/<id>/intent.lisp")
     (source external_app :shape "external service intent envelope for translation, code refactor, deploy ops, or other worker jobs"))
  :steps
    ((step review-question
       :question "What is the user or external application really asking MissionD to get done, and what facts are still unknown?"
       :writes [context-pack.questions context-pack.unknowns])
     (step normalize-intent
       :action "Normalize the source into work-order intent with objective, constraints, evidence refs, source app, privacy class, and optional source BoardTask.")
     (step board-anchor
       :action "Apply pre-task-board-anchor: every non-trivial mutation, deploy, skill edit, memory batch, or worker dispatch must bind the work-order to exactly one BoardTask before execution; create one only when the source is not already a BoardTask.")
     (step evidence-plan
       :question "Which evidence sources must be checked before deciding whether to dispatch a worker?"
       :writes [context-pack.evidence_needed])
     (step compile-plan
       :action "Compile plan.lisp with accepted shards, worker lane, read_scope, write_scope, risk gates, completion authority, and acceptance commands.")
     (step decompose-plan-to-shards
       :question "Which coarse plan steps can run independently, and which must stay ordered because they depend on earlier evidence or state?"
       :writes [plan-atomization-graph.shard_nodes plan-atomization-graph.dependency_edges])
     (step decompose-shards-to-atoms
       :action "Split each accepted shard into atom_tasks with atom_task_id, atom_path, predicted_tool_sequence, context_sources, read_scope, write_scope, acceptance, and detour_budget; a broad plan step is not dispatchable.")
     (step schedule-atom-graph
       :action "Mark every atom execution_order=serial or execution_order=parallel. Serial atoms lower to BoardTask dependsOn; parallel atoms lower to independent BoardTasks sharing a parallel_group.")
     (step prepare-controlled-work-area
       :action "For external Codex/ClaudeCode or customer-facing work, create or require a governed branch/worktree and .missiond/work-orders/<id>/ package so the agent cannot land code without the work-order gate.")
     (step verify-before-submit
       :action "Before commit or merge, run missiond-work-order verify --staged or missiond-work-order verify --commit <sha>; changed code files must be covered by accepted shard write_scope.")
     (step start-workflow-run
       :action "Start workflow_run and shared-memory cursors, record plan hash, and append audit header.")
     (step dispatch-workers
       :action "Dispatch only accepted atom tasks through BoardTask/Autopilot; broad objectives and coarse plan shards are not implementation prompts.")
     (step collect-results
       :action "Collect task-result-artifacts and map Board notes/provider finals/app callbacks as projections.")
     (step settle-close
       :action "Close or backfill after durable final settle, task-result artifact validation, audit append, slot release, and Lisp/checker convergence when code changed; provider workers do not own the primary BoardTask done transition.")
     (step promote-experience
       :question "Did this work-order expose a reusable operation that should become workflow.lisp, evidence, or a project constant?"
       :writes [workflow_promotion_candidate evidence_ref]))
  :external_app_policy
    ((class translation
       :intent_entry "external-app intent.lisp"
       :worker_lane "translation-or-router-worker"
       :rule "Translation workers receive input artifact refs and output task-result-artifacts; no PTY/progress text is canonical.")
     (class code_refactor
       :intent_entry "external-app intent.lisp"
       :worker_lane "claude-code-opus or narrow Sonnet shard"
       :rule "Refactor workers require accepted shard, write_scope, source compatibility, and no customer-facing MissionD work files in deliverables.")
     (class deploy_ops
       :intent_entry "operator or external deploy intent"
       :worker_lane "claude-code-deploy-ops"
       :rule "Cloud credentials are secret_ref only; operational steps write redacted audit and reusable deploy workflow evidence."))
  :risk-gates
    ((gate no-broad-implementation :rule "Implementation worker cannot start without context_pack_path, accepted_shard_id, and non-empty write_scope.")
     (gate grounding-required-before-worker :rule "Non-exact broad tasks cannot start any worker until mission_context_gather(persist=true) has produced a grounding_context_id and context_pack_path; missing grounding is a runtime block, not a prompt suggestion.")
     (gate plan-not-dispatchable :rule "plan.lisp is a route forecast and decomposition source only; workers receive atom_task_contracts produced by plan-atomization-graph.")
     (gate atom-graph-required :rule "Worker BoardTasks created from a plan must carry atom_task_id, atom_path, execution_order, dependency_policy, and either dependsOn serial edges or parallel_group metadata.")
     (gate serial-parallel-contract :rule "execution_order=serial must be represented by dependsOn/dependency_edges; execution_order=parallel must have no mutual sibling dependency and must share a parallel_group.")
     (gate provider-text-only-source :rule "ClaudeCode, Codex CLI, and Agy may assist plan decomposition only as text-only no-tools proposal sources; xjpcode remains the governed execution runtime.")
     (gate detour-telemetry-required :rule "If a worker searches beyond predicted_tool_sequence/context_sources or invents a subplan, record worker-detour-telemetry and create a decomposition/context improvement candidate.")
     (gate external-work-order-submit-gate :rule "Code changes from external conversations or local workers cannot be committed/merged/deployed unless intent.lisp, plan.lisp, MissionD-Work-Order id, accepted_shard_id, and write_scope coverage verify successfully. pre-commit validates staged code files against env/--id metadata; commit-msg validates the MissionD-Work-Order trailer after the message exists.")
     (gate no-secret-values :rule "intent.lisp, plan.lisp, Board notes, and audit.lisp may contain secret_ref only.")
     (gate board-intent-single-chain :rule "Board source and intent source share one workflow_run and one BoardTask anchor.")
     (gate result-artifact-required :rule "Done requires task-result-artifact or a documented no-output diagnostic.")
     (gate no-provider-self-close :rule "Provider prompts may surface BoardTask id for audit, but must not instruct workers to call mission_board_update(status=done) before task-result-artifact settlement; Board notes are projections only.")
     (gate external-app-same-chain :rule "External app jobs may not bypass BoardTask, shared-memory, task-result artifact, or audit surfaces."))
  :completion
    ((criterion done :rule "BoardTask done, task-result-artifact stored, audit.lisp appended, workflow_run closed.")
     (criterion blocked :rule "BoardTask blocked with missing evidence, approval, credential_ref, or unavailable worker lane.")
     (criterion backfill :rule "Code-first or external-app work created a visible Lisp/checker/evidence backfill task."))
  :checks ["node scripts/check-v3-work-order-lifecycle-isomorphism.mjs --json"])
