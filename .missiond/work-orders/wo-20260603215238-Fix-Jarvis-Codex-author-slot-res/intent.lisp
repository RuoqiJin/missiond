(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260603215238-Fix-Jarvis-Codex-author-slot-res"
  :objective "Fix Jarvis Codex author slot resolution so stale AGY author slot env cannot bind codex_cli authoring to the wrong provider slot"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
