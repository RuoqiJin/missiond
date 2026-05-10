(workflow multi-project-m6-wave
  :schema "missiond.workflow.multi-project-m6-wave.v1"
  :workflow_id multi-project-m6-wave
  :status active
  :owner resident-master-control
  :authority [project-m6-depth project-maturity-model workstation-pool]
  :source_plans [auth-m6-depth multi-project-maturity typed-lisp-compiler-convergence]
  :purpose "Batch projects from M5 worker-operational SSOT into Auth-grade M6 without relying on ad-hoc long prompts."
  :inputs [target-project-ids project-maturity-registry project-universe context-pack-root worker-pool acceptance-commands]
  :match_rules
    ((trigger :kind manual :tool mission_swarm_run :when "objective asks for projects to reach M6 or Auth-grade SSOT")
     (trigger :kind scheduled :workflow project-m6-depth :when "safe daytime batch, not nightly self-evolution")
     (trigger :kind boardtask :title-prefix "Run project M6 wave"))
  :steps
    ((step s1 :id select-wave
       :logic "Read project-maturity-registry and choose the highest-priority projects below M6; never use old M10 or H-level language.")
     (step s2 :id unknowns-first-intake
       :logic "For each selected project, ask what information is still unknown before judging intent or architecture, then map each unknown to SSOT, skill operational facts, code, deploy-center facts, EventBus evidence, checker output, or user decision.")
     (step s3 :id intent-inference
       :logic "Infer the user's project-level intent and durable preference for this wave, including why these projects matter now and what governance principle should be remembered.")
     (step s4 :id intent-memory-capture
       :logic "Write per-project or wave-level intent_memory_candidate artifacts with evidence_refs, confidence, and supersession_scope. Stable high-confidence intent is persisted as memory:decision through mission_kb_remember; uncertain intent remains needs-review/candidate for later MissionD consciousness evolution.")
     (step s5 :id review-question
       :logic "For each selected project, ask the resident-master style review question: is the SSOT Lisp granular enough, what architecture could be more elegant, and what evidence or workers are needed?")
     (step s6 :id evidence-plan
       :logic "Write a per-project context-pack plan containing questions, hypotheses, evidence_needed, read_scope, expected findings, and candidate worker lanes.")
     (step s7 :id investigation
       :logic "Create read-only survey BoardTasks for each selected project, using target project cwd/read_scope and durable conversation task attribution. Investigator prompts are question-first and return Findings / Evidence / Recommendations / Verification.")
     (step s8 :id synthesis
       :logic "Merge investigation artifacts into per-project findings, unresolved contradictions, and evidence confidence. Do not compile code shards while the synthesis has unresolved critical gaps.")
     (step s9 :id design-proposal
       :logic "For each project, produce design_options and choose an accepted M6 target architecture before implementation.")
     (step s10 :id exact-shards
       :logic "Compile accepted_shards for domain/policy/flow/event/runtime/implementation/compatibility/final-report with file or region ownership.")
     (step s11 :id implementation
       :logic "Dispatch Claude Opus for accepted implementation shards, Sonnet only for narrow exact patches, Gemini only for read-only summarization.")
     (step s12 :id verification
       :logic "Run project-local checks, check-project-maturity --min-level M6, and relevant focused tests before advancing maturity.")
     (step s13 :id update-registry
       :logic "Advance project-maturity-registry to M6 only after checker evidence passes; otherwise write the remaining gap and follow-up BoardTask.")
     (step s14 :id global-report
       :logic "Write a global M6 report with current maturity, design-only items, user decisions, and next code refactors."))
  :context-pack-artifacts [unknowns inferred_user_intent intent_memory_candidate questions hypotheses evidence_needed findings design_options accepted_shards]
  :egress [context-packs unknowns inferred-user-intent intent-memory-candidates questions hypotheses evidence_needed findings design_options accepted-shards worker-boardtasks maturity-registry-delta global-m6-report decision-items]
  :risk-gates
    ((gate g1 :rule "Lisp/checker changes precede runtime code changes.")
     (gate g2 :rule "External project tasks must use target project cwd/read_scope.")
     (gate g3 :rule "No production deploy, DNS mutation, secret mutation, or destructive migration.")
     (gate g4 :rule "No recursive cargo fmt or broad formatter runs.")
     (gate g5 :rule "M6 requires hot-path wiring and regression evidence, not M5 evidence alone.")
     (gate g6 :rule "No selected project may skip unknowns-first-intake, intent-inference, intent-memory-capture, review-question, or evidence-plan unless exact-shard-ready=true is explicit in the parent BoardTask."))
  :completion
    ((criterion c1 :rule "All selected projects have explicit M-level status and gap list.")
     (criterion c2 :rule "M6 projects pass check-project-maturity --min-level M6 and local project checks.")
     (criterion c3 :rule "Unfinished projects have concrete follow-up shards, not prose-only gaps.")))
