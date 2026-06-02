(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-deploy-active-ownership-guard"
  :objective "Harden MissionD blue-green deploy ownership guard"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
