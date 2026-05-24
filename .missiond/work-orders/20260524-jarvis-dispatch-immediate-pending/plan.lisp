(plan
  :id "20260524-jarvis-dispatch-immediate-pending"
  :accepted-shard "jarvis-plan-confirmed-dispatch-immediate-follow-handle"
  :write-scope [
    ".missiond/v3/shards/request-runtime.lisp"
    ".missiond/work-orders/20260524-jarvis-dispatch-immediate-pending/intent.lisp"
    ".missiond/work-orders/20260524-jarvis-dispatch-immediate-pending/plan.lisp"
    "crates/missiond-core/src/ws/server.rs"
    "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
    "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
    "scripts/generated/v3_contracts.d.ts"
    "scripts/generated/v3_contracts.mjs"
    "scripts/generated/v3_runtime_defaults.mjs"
  ]
  :acceptance [
    "Jarvis plan-confirmed dispatch returns board_task_created plus result_pending follow_payload without waiting for worker terminal state"
    "Follow requests remain the only public SSE path that waits for task-result-artifact"
    "Codex and Agy post-deploy smokes complete without client timeout"
    "static gates pass"
  ])
