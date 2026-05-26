(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260526150500-Clean-task-result-artifact"
  :objective "Normalize task-result-artifact content so canonical results exclude raw PTY progress and escaped screen captures while raw provider evidence remains diagnostic-only."
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
