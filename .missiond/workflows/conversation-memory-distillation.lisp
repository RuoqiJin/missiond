;; MissionD workflow: conversation memory distillation.
;;
;; Purpose: define what may become long-lived memory after logs and project
;; SSOT are stable. This is a design/observe-only workflow for now. It must not
;; mutate KB, create Board tasks, or run embedding/reranker jobs by default.

(workflow conversation-memory-distillation
  :schema "missiond.workflow.v1"
  :workflow_id conversation-memory-distillation
  :status designed
  :source_plans [memory-search-v2 project-ssot-universe resident-master-control]
  :match_rules
    ((trigger :kind manual :name memory-audit)
     (mode :default observe-only :allowed [observe-only candidate-inventory infrastructure-issue-inventory]))
  :inputs [conversation-log-sample project-registry project-ssot-registry user-long-term-preferences infra-issue-hints]
  :steps
    ((step s1 :name collect-safe-sample
       :logic "Read bounded conversation samples only after provider role attribution is stable; do not scan full history by default.")
     (step s2 :name classify-memory-candidates
       :logic "Classify durable facts worth remembering: project root/domain/runtime/deploy owner, stable architecture decisions, user long-term preferences, repeated infrastructure defects, tool capability boundaries, verified workflow lessons, and optimization ideas not yet ready for Board.")
     (step s3 :name discard-noise
       :logic "Reject temporary task progress, facts already covered by project SSOT Lisp, raw logs, repeated summaries, fixed one-off bug traces, and unverified speculation.")
     (step s4 :name map-to-destination
       :logic "Route candidates to project constants, project blueprint/evidence, Universe registry, workflow.lisp, infrastructure issue inventory, or candidate-memory report; active KB write is not a default egress.")
     (step s5 :name rank-later
       :logic "Defer FTS/embedding/reranker implementation until project SSOT and provider role attribution are stable; QWEN embedding and reranker are recorded as future memory-search-v2 runtime dependencies.")
     (step s6 :name write-report
       :logic "Write a candidate memory / infrastructure issue inventory report for review; do not create BoardTask until the old Board queue and project SSOT coverage are ready."))
  :egress [candidate-memory-report infrastructure-issue-inventory project-constant-candidates]
  :guardrails
    ((rule :id no-default-kb-write :text "Default workflow never writes or deletes KB entries.")
     (rule :id ssot-supersedes-memory :text "Facts already represented in project SSOT Lisp are marked superseded-by-lisp and excluded from active memory candidates.")
     (rule :id embedding-deferred :text "FTS, QWEN embedding, and reranker design are deferred to memory-search-v2 after log attribution and project SSOT convergence.")))
