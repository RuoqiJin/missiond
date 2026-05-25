(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260525124633-Fix-Jarvis-task-result-artifact-"
  :objective "Fix Jarvis task-result-artifact duplicate writes"
  :source external-codex
  :status accepted
  :unknowns ["duplicate task-result-artifact rows observed for a single Jarvis confirmed plan worker final"]
  :evidence_refs ["task_result_artifacts duplicate rows for BoardTask 3269b32d-cf67-4fe3-86ae-34fd5573f310"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
