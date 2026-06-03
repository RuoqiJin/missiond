(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260604060421-Agy-launch-model-selector"
  :objective "Promote AGY 1.0.5 --model from observed CLI capability into MissionD provider-box spawn/restart policy, combinable with AGY bypass launch, while preserving interactive PTY and durable-final semantics"
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs ["agy --help includes --model" "agy models lists Gemini 3.1 Pro (High)"]
  :constraints ["Lisp-first" "no headless AGY print/prompt" "verify model from screen_identity after spawn" "commit-through-work-order-gate"])
