(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260603031200-deploy-active-commit-guard"
  :objective "Guard MissionD active release from same-root stale commit deploys"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
