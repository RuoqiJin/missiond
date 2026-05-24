(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "jarvis-codex-durable-final-artifact"
  :objective "Fix Jarvis Codex durable final artifact settle"
  :source external-codex
  :status accepted
  :unknowns ["Codex worker durable final can land after the first Autopilot settle window; stale provider/progress summaries must not satisfy the task output contract." "Watchdog close currently may project a provider summary note without first writing task-result-artifact."]
  :evidence_refs [".missiond/v3/shards/request-runtime.lisp" ".missiond/v3/shards/workstation-runtime.lisp" "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" "crates/missiond-core/src/ws/server.rs"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
