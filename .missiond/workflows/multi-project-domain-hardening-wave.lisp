(workflow multi-project-domain-hardening-wave
  :schema "missiond.workflow.multi-project-domain-hardening-wave.v1"
  :workflow_id multi-project-domain-hardening-wave
  :status active
  :owner resident-master-control
  :authority [project-domain-hardening project-maturity-model workstation-pool]
  :source_plans [auth-v3-hardening multi-project-maturity typed-lisp-compiler-convergence]
  :purpose "Batch projects from M10 into Auth-grade H-level domain hardening without relying on ad-hoc long prompts."
  :inputs [target-project-ids domain-hardening-registry project-universe context-pack-root worker-pool acceptance-commands]
  :match_rules
    ((trigger :kind manual :tool mission_swarm_run :when "objective asks for all projects to reach Auth-grade H5")
     (trigger :kind scheduled :workflow project-domain-hardening :when "safe daytime batch, not nightly self-evolution")
     (trigger :kind boardtask :title-prefix "Run project domain hardening wave"))
  :steps
    ((step s1 :id select-wave
       :logic "Read project-domain-hardening-registry and choose the highest-priority projects below H5; never confuse M10 with H5.")
     (step s2 :id context-pack-survey
       :logic "Create read-only survey BoardTasks for each selected project, using target project cwd/read_scope and durable conversation task attribution.")
     (step s3 :id compile-hardening-shards
       :logic "For each project, compile exact domain/policy/flow/event/runtime/implementation/compatibility/final-report shards with file or region ownership.")
     (step s4 :id dispatch-implementation
       :logic "Dispatch Claude Opus for implementation shards, Sonnet only for narrow exact patches, Gemini only for read-only summarization.")
     (step s5 :id verify-project
       :logic "Run project-local checks, project-domain-hardening checker, and relevant focused tests before advancing H-level.")
     (step s6 :id update-registry
       :logic "Advance project-domain-hardening-registry only after checker evidence passes; otherwise write the remaining gap and follow-up BoardTask.")
     (step s7 :id global-report
       :logic "Write a global H-level report with M-level, H-level, design-only items, user decisions, and next code refactors."))
  :egress [context-packs accepted-shards worker-boardtasks hardening-registry-delta global-hardening-report decision-items]
  :risk-gates
    ((gate g1 :rule "Lisp/checker changes precede runtime code changes.")
     (gate g2 :rule "External project tasks must use target project cwd/read_scope.")
     (gate g3 :rule "No production deploy, DNS mutation, secret mutation, or destructive migration.")
     (gate g4 :rule "No recursive cargo fmt or broad formatter runs.")
     (gate g5 :rule "H5 requires hot-path wiring and regression evidence, not M10 evidence alone."))
  :completion
    ((criterion c1 :rule "All selected projects have explicit H-level status and gap list.")
     (criterion c2 :rule "H5 projects pass project-domain-hardening checker and local project checks.")
     (criterion c3 :rule "Unfinished projects have concrete follow-up shards, not prose-only gaps.")))
