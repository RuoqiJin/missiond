(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260528070300-Reuse-Jarvis-confirmed-artifacts"
  :objective "Reuse prior Jarvis intent/plan artifact ids during confirmation instead of re-authoring semantic drafts"
  :source codex
  :status draft
  :unknowns []
  :evidence_refs ["xjpcode live smoke exposed confirmation turns must remain bound to the reviewed artifacts"]
  :constraints ["Lisp-first" "no fallback content generation" "commit-through-work-order-gate"])
