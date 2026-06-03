(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602125231-wo-mcp-call-stdout-flush"
  :intent "wo-20260602125231-wo-mcp-call-stdout-flush"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602125231-wo-mcp-call-stdout-flush-shard-default"
       :read_scope ["."]
       :write_scope ["scripts/mission-mcp-call.mjs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260602125231-wo-mcp-call-stdout-flush/**"]
       :acceptance ["node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "node scripts/mission-mcp-call.mjs mission_context_gather <json args> preserves full JSON under nested execFileSync"])))
