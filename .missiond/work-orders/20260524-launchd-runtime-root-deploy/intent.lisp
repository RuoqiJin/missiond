(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260524-launchd-runtime-root-deploy"
  :objective "Fix MissionD managed-node blue-green deploy so launchd runtime root follows current Git/codebase root before restart"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
