(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "grounded-dispatch-runtime"
  :objective "Enforce grounded dispatch before broad worker tasks"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
