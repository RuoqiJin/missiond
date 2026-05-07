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
     (step s2 :id context-pack-survey
       :logic "Create read-only survey BoardTasks for each selected project, using target project cwd/read_scope and durable conversation task attribution.")
     (step s3 :id compile-m6-shards
       :logic "For each project, compile exact domain/policy/flow/event/runtime/implementation/compatibility/final-report shards with file or region ownership.")
     (step s4 :id dispatch-implementation
       :logic "Dispatch Claude Opus for implementation shards, Sonnet only for narrow exact patches, Gemini only for read-only summarization.")
     (step s5 :id verify-project
       :logic "Run project-local checks, check-project-maturity --min-level M6, and relevant focused tests before advancing maturity.")
     (step s6 :id update-registry
       :logic "Advance project-maturity-registry to M6 only after checker evidence passes; otherwise write the remaining gap and follow-up BoardTask.")
     (step s7 :id global-report
       :logic "Write a global M6 report with current maturity, design-only items, user decisions, and next code refactors."))
  :egress [context-packs accepted-shards worker-boardtasks maturity-registry-delta global-m6-report decision-items]
  :risk-gates
    ((gate g1 :rule "Lisp/checker changes precede runtime code changes.")
     (gate g2 :rule "External project tasks must use target project cwd/read_scope.")
     (gate g3 :rule "No production deploy, DNS mutation, secret mutation, or destructive migration.")
     (gate g4 :rule "No recursive cargo fmt or broad formatter runs.")
     (gate g5 :rule "M6 requires hot-path wiring and regression evidence, not M5 evidence alone."))
  :completion
    ((criterion c1 :rule "All selected projects have explicit M-level status and gap list.")
     (criterion c2 :rule "M6 projects pass check-project-maturity --min-level M6 and local project checks.")
     (criterion c3 :rule "Unfinished projects have concrete follow-up shards, not prose-only gaps.")))
