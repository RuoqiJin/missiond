(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-202605270002-jarvis-follow-short-poll"
  :objective "Fix Jarvis follow route timeout by making follow supervision short-poll resumable instead of long-held public stream"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
