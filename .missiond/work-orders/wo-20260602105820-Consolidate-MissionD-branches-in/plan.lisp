(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602105820-Consolidate-MissionD-branches-in"
  :intent "wo-20260602105820-Consolidate-MissionD-branches-in"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602105820-Consolidate-MissionD-branches-in-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["node scripts/compile-v3-runtime.mjs --json --check"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"])))
