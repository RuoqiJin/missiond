;; Wave 30 session trace.

(session-trace wave30
  :schema "missiond.session-trace.v1"
  :wave wave30
  :created-at "2026-04-28T16:15:28+08:00"
  :sequence 1

  (trace-event
    :id wave30-trace-bootstrap-001
    :seq 1
    :at "2026-04-28T16:15:28+08:00"
    :task wave30-02-staged-source-hygiene-v0
    :backend codex-orchestrator
    :kind dispatch
    :summary "Wave30 generated as productive-only lifecycle-finalization wave. Thin briefs point at shared preamble, context atlas, pattern cards, and GPT Pro/Codex action plan before broad repository search.")

  (trace-event
    :id wave30-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-28T08:15:47Z"
    :task wave30-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave30-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-28T08:15:47Z"
    :task wave30-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave30-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
