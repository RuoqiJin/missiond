(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603010344-Jarvis-MCP-grounding-gate-and-Ma"
  :intent "wo-20260603010344-Jarvis-MCP-grounding-gate-and-Ma"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603010344-Jarvis-MCP-grounding-gate-and-Ma-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/lib.rs"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/provider_box/claude_code_driver.rs"
                     "crates/missiond-daemon/src/provider_box/http_adapter.rs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
