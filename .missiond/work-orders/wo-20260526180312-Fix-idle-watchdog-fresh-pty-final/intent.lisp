(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260526180312-Fix-idle-watchdog-fresh-pty-final"
  :objective "Fix idle watchdog to prefer fresh PTY screen over stale cached progress when extracting task-result-artifact"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
