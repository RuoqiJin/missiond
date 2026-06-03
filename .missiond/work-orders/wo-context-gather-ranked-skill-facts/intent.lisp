(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-context-gather-ranked-skill-facts"
  :objective "Rank and summarize deploy_ops skill operational facts inside mission_context_gather so compact lanes expose relevant deployment evidence without raw-source noise"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
