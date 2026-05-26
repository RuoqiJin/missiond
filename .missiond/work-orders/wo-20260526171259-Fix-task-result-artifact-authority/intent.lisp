(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260526171259-Fix-task-result-artifact-authority"
  :objective "Fix Jarvis/Autopilot task-result-artifact authority so intermediate progress or Board summary notes cannot close client-channel tasks"
  :source external-codex
  :status draft
  :unknowns ["Which runtime path synthesized the bad result artifact for Mac mini BoardTask 265e6ebb?"]
  :evidence_refs ["Mac mini task 265e6ebb produced task-result-artifact from an intermediate progress line while PTY later showed real Findings/Evidence output was still being generated"]
  :constraints ["Lisp-first" "artifact-first-completion" "no Board note as completion authority" "commit-through-work-order-gate"])
