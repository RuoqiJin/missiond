(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260602201823-runtime-evidence-freshness"
  :objective "Filter stale persisted runtime_environment evidence from mission_context_gather default retrieval"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "runtime-read-model-freshness" "no-secret-values" "commit-through-work-order-gate"])
