(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603233451-Expose-Jarvis-provider-box-slot-"
  :intent "wo-20260603233451-Expose-Jarvis-provider-box-slot-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603233451-Expose-Jarvis-provider-box-slot--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/smoke-jarvis-chain.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
