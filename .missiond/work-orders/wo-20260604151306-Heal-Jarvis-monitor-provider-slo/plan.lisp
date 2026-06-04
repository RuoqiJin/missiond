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
                     "crates/missiond-core/src/ws/server.rs"]
       :acceptance ["node scripts/check-v3-interaction-gateway-isomorphism.mjs"
                    "cargo check -p missiond-core"])))
