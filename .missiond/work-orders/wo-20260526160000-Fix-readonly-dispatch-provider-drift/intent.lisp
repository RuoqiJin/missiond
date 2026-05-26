(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260526160000-Fix-readonly-dispatch-provider-drift"
  :objective "Fix Jarvis read-only dispatch classification and provider enum drift found by Mac mini MissionD client-channel smoke."
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs ["BoardTask 6a5cbbbe-58f3-4aa6-a032-b26d1575b701"
                  "task-result-artifact 8eb1437cb5dd7b34a1ee73502aa82b66779e241b872e3eaf45b15ec0ba7eee8b"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate" "do-not-use-rsync"])
