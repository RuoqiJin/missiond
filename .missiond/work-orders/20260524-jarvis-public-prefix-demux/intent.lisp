(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260524-jarvis-public-prefix-demux"
  :objective "Fix Jarvis public /jarvis/* HTTP demux so auth proxy paths do not fall through to WebSocket handling"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
