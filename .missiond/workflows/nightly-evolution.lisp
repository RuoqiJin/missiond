;; MissionD workflow: nightly evolution loop.
;;
;; Purpose: make resident Codex master review MissionD's own Lisp/runtime
;; loops on a schedule, without relying on an ad-hoc prompt. The workflow is
;; observe-first: it writes a structured report plus self-evolution proposal
;; artifacts and creates visible review BoardTasks only when apply=true.

(workflow nightly-evolution
  :schema "missiond.workflow.v1"
  :workflow_id nightly-evolution
  :status manual-first
  :source_plans [resident-master-control v3-night-scheduler]
  :match_rules
    ((trigger :kind schedule :name nightly-evolution :default disabled :enable-env MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE)
     (trigger :kind manual :tool mission_nightly_evolution)
     (mode :default observe-only :allowed [observe-only safe-backfill needs-investigation architecture-proposal requires-user-decision]))
  :inputs [ssot-retrieval-scope.active-authoring missiond-v3-blueprint compiled-semantic-ir compiled-workflows final-convergence-static-snapshot deterministic-self-evolution-analyzer]
  :steps
    ((step s1 :name collect-evidence
       :logic "Read only ssot-retrieval-scope active-authoring MissionD V3 files, compiled-semantic-ir, compiled-workflows, V3 checker output, and final convergence static snapshot. Default nightly mode does not read KB, historical conversations, provider logs, worker telemetry, Board open tasks, or recent commit history, and also excludes cold runtime artifacts except the compiled runtime JSON inputs.")
     (step s2 :name detect-loop-smells
       :logic "Run scripts/analyze-v3-self-evolution.mjs --json to deterministically detect final-convergence-blocker, facade-budget-near-limit, oversized-authoring-block, and surface-flow-gap findings.")
     (step s3 :name classify-risk
       :logic "Classify each analyzer finding as safe-backfill, needs-investigation, architecture-proposal, or requires-user-decision and keep analyzer diagnostics visible instead of silently succeeding.")
     (step s4 :name write-report
       :logic "Write .missiond/v3/runtime/nightly-evolution/<date>.report.lisp with evidence refs, analyzer diagnostics, proposal-paths, findings, and no code mutation.")
     (step s5 :name create-followups
       :logic "Write at most three .missiond/v3/runtime/self-evolution/<timestamp>-<finding_id>.proposal.lisp artifacts, sorted by low risk first then finding id; proposal fields are :proposal_id :finding_id :class :risk :summary :evidence_refs :affected_surfaces :recommended_change :acceptance :non_goals :created_at.")
     (step s6 :name proposal-review-task
       :logic "When apply=true, create exactly one visible review BoardTask for the selected proposal paths, with auto_execute=false; do not create exact shards or worker implementation tasks.")
     (step s7 :name checkpoint
       :logic "Update resident master checkpoint so daemon restart can resume the next night; no KB task or memory mutation is created in default mode."))
  :artifacts
    ((artifact self-evolution-proposal
       :schema "missiond.self-evolution-proposal.v1"
       :path ".missiond/v3/runtime/self-evolution/<timestamp>-<finding_id>.proposal.lisp"
       :required [:proposal_id :finding_id :class :risk :summary :evidence_refs :affected_surfaces :recommended_change :acceptance :non_goals :created_at]
       :writer mission_nightly_evolution
       :ssot false
       :rule "Proposal artifacts are runtime evidence for review only; they are ignored by git and never mutate code, Lisp, checker files, KB, Board history, provider logs, or worker telemetry by themselves."))
  :egress [nightly-evolution-report self-evolution-proposal proposal-review-boardtask master-control-checkpoint]
  :risk-gates
    ((gate g1 :rule "Default mode writes report/proposal artifacts only and does not mutate code, Lisp, checker files, or KB.")
     (gate g2 :rule "Default evidence is restricted to ssot-retrieval-scope active-authoring files, compiled-semantic-ir, compiled-workflows, V3 checker output, and static convergence snapshot; KB, historical conversations, provider logs, worker telemetry, Board open tasks, and recent commit history are excluded.")
     (gate g3 :rule "Proposal review BoardTasks are visible, auto_execute=false, and never hide/delete historical tasks.")
     (gate g4 :rule "Scheduled runs are opt-in while active supervision is ongoing."))
  :completion
    ((criterion c1 :rule "A nightly report is written with analyzer findings classified by risk and proposal-paths.")
     (criterion c2 :rule "At most three self-evolution proposal artifacts are written, sorted low risk first then finding id.")
     (criterion c3 :rule "No KB, provider log, historical conversation, Board-open-task input, worker telemetry, or recent commit history is used in default mode.")
     (criterion c4 :rule "Resident master checkpoint records the report path and next scheduled/resume state."))
  :guardrails
    ((rule :id observe-first :text "Default mode writes report/proposal artifacts only.")
     (rule :id visible-tasks :text "The workflow may create one visible review BoardTask when apply=true; it must not auto-execute, hide, delete, or bulk-mutate historical tasks.")
     (rule :id no-direct-code :text "Resident master does not edit code directly; code changes require separate human-approved exact write-scope BoardTasks.")
     (rule :id schedule-opt-in :text "Scheduled nightly evolution is disabled by default during active supervision; manual mission_nightly_evolution remains available, and periodic runs require MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE=true.")
     (rule :id v3-only-default :text "Default nightly mode is an active-authoring SSOT Lisp plus compiled-runtime review; PTY, KB, provider logs, Board history, worker telemetry, recent commit history, cold runtime reports, and historical conversation evidence belong to explicit follow-up workflows.")))
