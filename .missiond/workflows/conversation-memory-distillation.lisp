;; MissionD workflow: conversation memory distillation.
;;
;; Purpose: define what may become long-lived memory after logs and project
;; SSOT are stable. This is a design/observe-only workflow for now. It must not
;; mutate KB, create Board tasks, or run embedding/reranker jobs by default.

(workflow conversation-memory-distillation
  :schema "missiond.workflow.v1"
  :workflow_id conversation-memory-distillation
  :status designed
  :source_plans [memory-search-v2 project-ssot-universe resident-master-control memory-review-batch-runner]
  :match_rules
    ((trigger :kind manual :name memory-audit)
     (mode :default observe-only :allowed [observe-only candidate-inventory infrastructure-issue-inventory]))
  :inputs [conversation-log-sample project-registry project-ssot-registry ssot-retrieval-scope.active-authoring user-long-term-preferences infra-issue-hints]
  :steps
    ((step s1 :name collect-safe-sample
       :logic "Read bounded conversation samples only after provider role attribution is stable; use ssot-retrieval-scope active-authoring files for supersession checks, and do not scan full history or cold runtime artifacts by default.")
     (step s2 :name calibrate-review-rubric
       :logic "Before any batch cleanup, Codex or master manually reviews at least five batches of about ten memories, records keep/archive/delete-candidate examples, and tunes the review rubric.")
     (step s3 :name classify-memory-candidates
       :logic "Classify durable facts worth remembering: project root/domain/runtime/deploy owner, stable architecture decisions, user long-term preferences, repeated infrastructure defects, tool capability boundaries, verified workflow lessons, and optimization ideas not yet ready for Board.")
     (step s4 :name discard-noise
       :logic "Reject temporary task progress, facts already covered by project SSOT Lisp, raw logs, repeated summaries, fixed one-off bug traces, and unverified speculation.")
     (step s5 :name map-to-destination
       :logic "Route candidates to project constants, project blueprint/evidence, Universe registry, workflow.lisp, infrastructure issue inventory, or candidate-memory report; active KB write is not a default egress.")
     (step s6 :name review-overlay
       :logic "Write only knowledge_review_state overlay by default: active, superseded-by-lisp, superseded-by-code, historical-evidence, duplicate, wrong-or-stale, delete-candidate, or needs-human. Do not mutate the original knowledge rows.")
     (step s7 :name rank-later
       :logic "Defer FTS/embedding/reranker implementation until project SSOT and provider role attribution are stable; QWEN embedding and reranker are recorded as future memory-search-v2 runtime dependencies.")
     (step s8 :name write-report
       :logic "Write a candidate memory / infrastructure issue inventory report for review; do not create BoardTask until the old Board queue and project SSOT coverage are ready."))
  :egress [candidate-memory-report infrastructure-issue-inventory project-constant-candidates memory-review-batch-runner-manifest]
  :risk-gates
    ((gate g1 :rule "Default mode is observe-only and never writes/deletes active KB.")
     (gate g2 :rule "Conversation samples are bounded and provider role attribution must be stable before any candidate extraction.")
     (gate g3 :rule "Facts already represented in project SSOT active-authoring Lisp are excluded from active memory candidates; cold runtime reports are evidence only when explicitly referenced.")
     (gate g4 :rule "Batch triage requires a manual pilot of at least five ten-item batches and targets about 10% active memory.")
     (gate g5 :rule "Embedding, QWEN, and reranker jobs remain deferred until memory-search-v2 is explicitly enabled."))
  :completion
    ((criterion c1 :rule "The workflow can produce a candidate-memory report without mutating KB or Board.")
     (criterion c2 :rule "Each candidate is routed to project constants, blueprint/evidence, Universe registry, workflow Lisp, or infrastructure issue inventory.")
     (criterion c3 :rule "Rejected noise classes are explicit and machine-readable.")
     (criterion c4 :rule "Default KB retrieval excludes needs-human and archived review states while include_archived remains available for tracing.")
     (criterion c5 :rule "Large-scale review is delegated to memory-review-batch-runner so batch cursor, worker reports, task-result artifacts, and review overlay writes are checkpointed."))
  :guardrails
    ((rule :id no-default-kb-write :text "Default workflow never writes or deletes KB entries.")
     (rule :id ssot-supersedes-memory :text "Facts already represented in project SSOT Lisp are marked superseded-by-lisp and excluded from active memory candidates.")
     (rule :id embedding-deferred :text "FTS, QWEN embedding, and reranker design are deferred to memory-search-v2 after log attribution and project SSOT convergence.")))
