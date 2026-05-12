(workflow mechanic-repair-lane
  :schema "missiond.workflow.v1"
  :workflow_id mechanic-repair-lane
  :status active
  :owner missiond
  :purpose "Use Jarvis Mechanic as a narrow repair executor after MissionD has already discovered, reviewed, and accepted an exact shard."
  :source_plans [mechanic-collaboration-boundary project-m6-depth semantic-ir-shared-memory-convergence]
  :match-rules
  ((trigger checker-failure :requires [accepted_shard_id context_pack_path write_scope acceptance])
   (trigger commit-lisp-drift :requires [accepted_shard_id context_pack_path write_scope acceptance])
   (trigger repair-shard-approved :requires [accepted_shard_id context_pack_path write_scope acceptance])
   (reject broad-objective :when "accepted_shard_id missing or write_scope empty")
   (reject architecture-review :when "task asks mechanic to decide architecture or read Board/KB/provider logs broadly"))
  :steps
  ((step s1 :id discover-issue
    :entry [checker EventBus audit commit-lisp-convergence boardtask]
    :logic "MissionD detects a concrete defect or drift and writes an issue-candidate artifact; Mechanic is not involved in discovery."
    :egress [issue-candidate-artifact])
   (step s2 :id master-review
    :entry [issue-candidate-artifact ssot lisp-checker-output code-anchor]
    :logic "Resident master decides whether the issue is stale, a user decision, a broad design problem, or a repairable exact shard."
    :egress [stale-resolution decision-item accepted-shard-draft])
   (step s3 :id exact-shard-acceptance
    :entry [accepted-shard-draft]
    :logic "MissionD materializes context_pack_path, accepted_shard_id, write_scope, must_not_touch, acceptance, rollback policy, and shared-memory write lease."
    :egress [accepted_shard context_pack_path shared_claims])
   (step s4 :id delegate-mechanic
    :entry [accepted_shard context_pack_path shared_claims]
    :logic "Call mission_task_delegate with engine_hint=mechanic. Default mechanic_mode is dry-run; mechanic_mode=repair must be explicit. Standalone mission_mechanic tools are forbidden."
    :egress [mechanic-boardtask mechanic-subprocess])
   (step s5 :id normalize-result
    :entry [mechanic-subprocess stdout stderr exit-code]
    :logic "Normalize mechanic output into task-result-artifact; Board note and PTY are projections only."
    :egress [task-result-artifact])
   (step s6 :id missiond-verification
    :entry [task-result-artifact write_scope acceptance]
    :logic "MissionD verifies diff scope, checker/test acceptance, commit/Lisp convergence, and releases shared-memory claims before closing the BoardTask."
    :egress [verification-result BoardTaskClosed claims-released]))
  :risk-gates
  ((gate g1 :rule "Mechanic MUST NOT run without accepted_shard_id, context_pack_path, write_scope, and acceptance.")
   (gate g2 :rule "Mechanic MUST NOT subscribe to EventBus, Board, KB, or provider logs as an architect.")
   (gate g3 :rule "Mechanic repair mode must be explicit; dry-run is the default.")
   (gate g4 :rule "MissionD, not Mechanic, owns final verification and BoardTask closure.")
   (gate g5 :rule "PTY/workstation management stays in MissionD; Mechanic is a subprocess executor lane, not a PTY slot."))
  :completion
  ((criterion c1 :rule "A mechanic run produces exactly one task-result-artifact for the delegated BoardTask.")
   (criterion c2 :rule "Shared-memory write claims are released after worker-completion-settle.")
   (criterion c3 :rule "Acceptance command results are recorded in the task-result-artifact or follow-up verification artifact.")
   (criterion c4 :rule "Broad architecture findings become new issue-candidate artifacts, never mechanic self-delegation.")))
