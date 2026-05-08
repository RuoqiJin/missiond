(workflow semantic-ir-shared-memory-convergence
  :schema "missiond.workflow.semantic-ir-shared-memory-convergence.v1"
  :workflow_id semantic-ir-shared-memory-convergence
  :status active
  :owner resident-master-control
  :authority [typed-lisp-compiler mission-shared-memory project-ssot-convergence]
  :source_plans [semantic-compression concurrent-agent-execution]
  :purpose "Converge MissionD onto compact semantic IR for agent context and durable shared memory for concurrent worker coordination."
  :inputs [v3-blueprint workflow-lisp compiled-semantic-ir shared-events shared-artifacts shared-claims agent-cursors EventBus]
  :match_rules
    ((trigger :kind manual :surface mission-shared-memory :when "objective asks to improve semantic compression or concurrent agent coordination")
     (trigger :kind checker :code SHARED_MEMORY_ISOMORPHISM_FAILED)
     (trigger :kind runtime :event shared_memory_claim_conflict))
  :steps
    ((step s1 :id language-boundary-review
       :logic "Review whether Lisp, OCaml, Rust, and JS are each in their sweet spot: Lisp authors facts, OCaml compiles/types/compresses, Rust coordinates runtime concurrency, JS wraps and scans code anchors.")
     (step s2 :id semantic-ir-compile
       :logic "Run missiond-lispc emit-semantic-ir and scripts/compile-v3-runtime.mjs so agents receive compact semantic facts with source maps.")
     (step s3 :id shared-memory-prepare
       :logic "Ensure shared_events, shared_artifacts, shared_claims, and agent_cursors are available as the durable coordination store.")
     (step s4 :id context-slice-before-worker
       :logic "Before dispatching a worker, create or fetch a mission_context_slice containing goal, relevant facts, accepted shard, read/write scope, and acceptance commands.")
     (step s5 :id investigation-artifact
       :logic "Investigation workers write findings/reports as shared_artifacts and append shared_events; they do not need write leases.")
     (step s6 :id implementation-claim
       :logic "Implementation workers must have accepted_shard_id plus shared_claims leases for every write_scope before BoardTask dispatch.")
     (step s7 :id conflict-resolution
       :logic "If claims conflict, create a conflict artifact for the master/integrator; do not let workers race on the same file/region/surface.")
     (step s8 :id completion-evidence
       :logic "Worker completion is accepted only from durable artifact/event evidence; PTY idle remains diagnostic."))
  :egress [compiled-semantic-ir compiled-agent-slices context-slice shared-artifact shared-claim conflict-artifact convergence-report]
  :risk-gates
    ((gate g1 :rule "Generated compiled JSON must be produced by OCaml/compile scripts, never edited by hand.")
     (gate g2 :rule "Implementation BoardTasks without accepted shard and write lease are refused by runtime guard.")
     (gate g3 :rule "Legacy shared-memory.lisp is compatibility projection only and must not be used as concurrent write truth.")
     (gate g4 :rule "EventBus is wakeup/observation; shared-memory tables are durable coordination truth."))
  :completion
    ((criterion c1 :rule "check-v3-shared-memory-isomorphism passes.")
     (criterion c2 :rule "mission_shared_memory, mission_context_slice, and mission_claim_status MCP tools are registered.")
     (criterion c3 :rule "mission_master_status shows sharedMemory health.")
     (criterion c4 :rule "compile-v3-runtime emits semantic IR, agent slices, and workflow contracts.")))
