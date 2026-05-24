(plan
  :id "20260524-jarvis-db-poll-failfast"
  :accepted-shard "jarvis-public-sse-db-poll-timeout"
  :write-scope [
    ".missiond/v3/shards/request-runtime.lisp"
    ".missiond/work-orders/20260524-jarvis-db-poll-failfast/intent.lisp"
    ".missiond/work-orders/20260524-jarvis-db-poll-failfast/plan.lisp"
    "crates/missiond-core/src/ws/server.rs"
    "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
    "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
    "scripts/generated/v3_contracts.d.ts"
    "scripts/generated/v3_contracts.mjs"
    "scripts/generated/v3_runtime_defaults.mjs"
  ]
  :acceptance [
    "Jarvis BoardTask polling is bounded with typed diagnostics on timeout"
    "Jarvis notes polling cannot hang follow streams indefinitely"
    "Agy follow smoke can retrieve final task-result-artifact after completion"
    "static gates pass"
  ])
