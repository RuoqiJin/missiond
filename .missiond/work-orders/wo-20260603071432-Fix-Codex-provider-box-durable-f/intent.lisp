(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260603071432-Fix-Codex-provider-box-durable-f"
  :objective "Fix Codex provider-box durable final settle and Jarvis replay diagnostics for plan authoring."
  :source external-codex
  :status accepted
  :unknowns ["Public Jarvis edge timeout still needs end-to-end validation after deploy."]
  :evidence_refs ["provider-box returned PROVIDER_DURABLE_FINAL_MISSING at 19s while Codex session wrote a valid final JSON at 31s" "Jarvis replay showed key_judgment_draft but no plan or diagnostic event for the failed plan authoring turn"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
