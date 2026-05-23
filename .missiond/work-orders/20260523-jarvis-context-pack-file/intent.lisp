(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260523-jarvis-context-pack-file"
  :objective "Materialize Jarvis grounding context packs so provider CLI workers without MissionD MCP can execute grounded tasks"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["BoardTask 22673f17-1884-44e4-b628-46080282f5d1 fast-failed because Agy worker had no mission_shared_memory MCP access"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate" "GitHub-sync-to-macmini-no-rsync"])
