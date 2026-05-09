(workflow missiond-control-plane-m6-split
  :schema "missiond.workflow.v1"
  :workflow_id missiond-control-plane-m6-split
  :status active
  :source_plans [missiond-v3-runtime-ssot control-plane-m6-split typed-lisp-compiler-cleanup]
  :match_rules
    ((trigger :kind manual :tool mission_swarm_run :when "objective asks to review MissionD V3 control-plane granularity or split overloaded control-plane policy blocks")
     (trigger :kind boardtask :title-prefix "MissionD control-plane M6 split")
     (dedupe-key "missiond-control-plane-m6-split:<objective_id>"))
  :owner missiond
  :purpose "Turn MissionD's growing V3 control plane into small named subplanes before adding more orchestration, worker, deployment, skill, memory, or long-running batch behavior."
  :inputs [missiond-blueprint.lisp compiled-v3-blueprint compiled-workflows final-convergence-snapshot control-plane-m6-split]
  :entry [mission_nightly_evolution mission_swarm_run check-v3-control-plane-m6-split]
  :steps
    ((step s1 :id review-question
       :logic "Ask the resident master to review only MissionD V3 SSOT control-plane granularity: which blocks are too large, which loops are ambiguous, and which rules are not named functions.")
     (step s2 :id inventory-overloaded-blocks
       :logic "Measure workstation-config, resident-master-control, EventBridge/deployment, project-universe, memory/skill, and execution workflow blocks by function names, steps, long invariants, batch checkpoint ownership, and runtime projection ownership.")
     (step s3 :id define-subplanes
       :logic "Represent each overloaded area as a named domain with owner, source blocks, functions, runtime projection, checker pins, and a refactor rule.")
     (step s4 :id preserve-runtime-compatibility
       :logic "Keep existing runtime-read blocks in place until compiled JSON cutover is explicit; the split is a structural SSOT index, not a breaking runtime relocation.")
     (step s5 :id add-checker-pins
       :logic "Add or update checker pins so future changes must attach new rules to one control-plane domain instead of adding free-floating prose.")
     (step s6 :id compile-projection
       :logic "Regenerate compiled V3/universe/workflow JSON and compare source hashes so daemon/runtime projection can observe the new split.")
     (step s7 :id report-residual-gaps
       :logic "Write remaining gaps: which domains still need physical Rust/JS module split, which are Lisp-only structure, and which belong to future project M6 work."))
  :egress [control-plane-split-report checker-pins compiled-runtime-projection-gaps follow-up-shards]
  :risk-gates
    ((gate g1 :rule "Do not move runtime-owned fields out of existing Lisp blocks until Rust runtime loaders are explicitly updated.")
     (gate g2 :rule "Do not add new worker/deploy/autopilot behavior as a long invariant without a matching control-plane domain/function.")
     (gate g3 :rule "Historical methodology workflows remain reference artifacts; active workflow dispatch must use missiond.workflow.v1 contracts.")
     (gate g4 :rule "No production deploy, daemon restart, or MCP mutation is required for this workflow."))
  :completion
    ((criterion c1 :rule "control-plane-m6-split contains six named domains: workstation, master, eventbridge/deployment, project/universe, knowledge/skill, and execution.")
     (criterion c2 :rule "Each domain declares owner, source, functions, runtime-projection, checker, and refactor-rule.")
     (criterion c3 :rule "check-v3-control-plane-m6-split and final convergence static gate pass.")
     (criterion c4 :rule "Compiled workflow projection includes missiond-control-plane-m6-split.")))
