;; MissionD workflow: nightly evolution loop.
;;
;; Purpose: make resident Codex master review MissionD's own Lisp/runtime
;; loops on a schedule, without relying on an ad-hoc prompt. The workflow is
;; observe-first: it writes a structured report and creates visible follow-up
;; BoardTasks only for low-risk, explicit work.

(workflow nightly-evolution
  :schema "missiond.workflow.v1"
  :workflow_id nightly-evolution
  :status active
  :source_plans [resident-master-control v3-night-scheduler]
  :match_rules
    ((trigger :kind schedule :name nightly-evolution)
     (trigger :kind manual :tool mission_nightly_evolution)
     (mode :default observe-only :allowed [observe-only safe-backfill needs-investigation architecture-proposal requires-user-decision]))
  :inputs [v3-blueprint frontend-board-blueprint project-registry board-open-tasks recent-events recent-commits worker-telemetry final-convergence-snapshot]
  :steps
    ((step s1 :name collect-evidence
       :logic "Read V3/project Lisp, Board open task summary, event tail, provider durable logs, recent commits, worker telemetry, and final convergence static snapshot.")
     (step s2 :name detect-loop-smells
       :logic "Check commit-Lisp drift, missing event subscriptions, text-summary classifiers, legacy direct workers, PTY-only completion decisions, Board/Autopilot close authority, frontend cockpit visibility, and long repeated Lisp prose.")
     (step s3 :name classify-risk
       :logic "Classify each finding as observe-only, safe-backfill, needs-investigation, architecture-proposal, or requires-user-decision.")
     (step s4 :name write-report
       :logic "Write .missiond/v3/runtime/nightly-evolution/<date>.report.lisp with evidence refs, findings, proposed follow-ups, and no code mutation.")
     (step s5 :name create-followups
       :logic "Create visible BoardTasks for safe-backfill or investigation findings; high-risk architecture changes become proposal tasks only.")
     (step s6 :name optional-readonly-swarm
       :logic "When apply=true and a finding is needs-investigation, call mission_swarm_run in read-only mode with exact project, context, timeout, and acceptance.")
     (step s7 :name checkpoint
       :logic "Update resident master checkpoint and KB summary so daemon restart can resume the next night."))
  :egress [nightly-evolution-report boardtask kb-note master-control-checkpoint]
  :guardrails
    ((rule :id observe-first :text "Default mode writes report only.")
     (rule :id visible-tasks :text "The workflow may create visible BoardTasks; it must not hide, delete, or bulk-mutate historical tasks.")
     (rule :id no-direct-code :text "Resident master does not edit code directly; code changes require exact write-scope BoardTasks.")
     (rule :id pty-diagnostic-only :text "PTY state can explain slot health but cannot be sole completion authority.")))
