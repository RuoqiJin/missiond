(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260527110000-deploy-socket-readiness"
  :objective "Make MissionD blue-green deploy readiness use MCP initialize instead of lsof socket ownership"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
