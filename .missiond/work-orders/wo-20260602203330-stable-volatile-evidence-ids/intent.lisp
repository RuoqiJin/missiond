(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260602203330-stable-volatile-evidence-ids"
  :objective "Prevent volatile context_gather runtime/support compact evidence from accumulating stale content-hash IDs across deploys"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-raw-history-deletion" "read-model-noise-reduction" "commit-through-work-order-gate"])
