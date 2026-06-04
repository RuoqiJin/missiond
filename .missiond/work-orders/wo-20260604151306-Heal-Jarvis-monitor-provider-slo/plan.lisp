(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260604151306-Heal-Jarvis-monitor-provider-slo"
  :intent "wo-20260604151306-Heal-Jarvis-monitor-provider-slo"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260604151306-Heal-Jarvis-monitor-provider-slo-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/request-surfaces.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/deploy-daemon.sh"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-interaction-gateway-isomorphism.mjs"
                    "cargo check -p missiond-core"])))
