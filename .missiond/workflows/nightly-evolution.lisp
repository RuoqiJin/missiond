;; MissionD workflow: nightly evolution loop.
;;
;; Purpose: make resident Codex master review MissionD's own Lisp/runtime
;; loops on a schedule, without relying on an ad-hoc prompt. The workflow is
;; observe-first: it writes a structured report and creates visible follow-up
;; BoardTasks only for low-risk, explicit work.

(workflow nightly-evolution
  :schema "missiond.workflow.v1"
  :workflow_id nightly-evolution
  :status manual-first
  :source_plans [resident-master-control v3-night-scheduler]
  :match_rules
    ((trigger :kind schedule :name nightly-evolution :default disabled :enable-env MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE)
     (trigger :kind manual :tool mission_nightly_evolution)
     (mode :default observe-only :allowed [observe-only safe-backfill needs-investigation architecture-proposal requires-user-decision]))
  :inputs [missiond-v3-blueprint v3-surface-checkers final-convergence-static-snapshot recent-v3-commits]
  :steps
    ((step s1 :name collect-evidence
       :logic "Read only MissionD V3 blueprint, V3 checker output, final convergence static snapshot, and recent commits that touched .missiond/v3/**. Default nightly mode does not read KB, historical conversations, provider logs, worker telemetry, or Board open tasks.")
     (step s2 :name detect-loop-smells
       :logic "Check only MissionD V3 SSOT logic: contradictory loops, structure repetition, surface/checker gaps, runtime projection gaps, missing entry/core/egress steps, and long repeated Lisp prose.")
     (step s3 :name classify-risk
       :logic "Classify each finding as observe-only, safe-backfill, needs-investigation, architecture-proposal, or requires-user-decision.")
     (step s4 :name write-report
       :logic "Write .missiond/v3/runtime/nightly-evolution/<date>.report.lisp with evidence refs, findings, proposed follow-ups, and no code mutation.")
     (step s5 :name create-followups
       :logic "When apply=true, select only findings whose class matches the requested mode; never fall back from needs-investigation or proposal modes to safe-backfill.")
     (step s6 :name optional-readonly-swarm
       :logic "When the selected finding is needs-investigation, create a read-only V3 SSOT context organizer follow-up or call mission_swarm_run in read-only mode with exact project, context, timeout, and acceptance.")
     (step s7 :name checkpoint
       :logic "Update resident master checkpoint so daemon restart can resume the next night; no KB task or memory mutation is created in default mode."))
  :egress [nightly-evolution-report boardtask master-control-checkpoint]
  :guardrails
    ((rule :id observe-first :text "Default mode writes report only.")
     (rule :id visible-tasks :text "The workflow may create visible BoardTasks; it must not hide, delete, or bulk-mutate historical tasks.")
     (rule :id no-direct-code :text "Resident master does not edit code directly; code changes require exact write-scope BoardTasks.")
     (rule :id schedule-opt-in :text "Scheduled nightly evolution is disabled by default during active supervision; manual mission_nightly_evolution remains available, and periodic runs require MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE=true.")
     (rule :id v3-only-default :text "Default nightly mode is an SSOT Lisp review; PTY, KB, provider logs, and historical conversation evidence belong to explicit follow-up workflows.")))
