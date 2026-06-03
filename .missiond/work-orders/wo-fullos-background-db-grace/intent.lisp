(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-fullos-background-db-grace"
  :objective "Defer full-os background DB maintenance after deploy so MissionD search stays responsive"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
