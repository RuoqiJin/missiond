(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531101448-Refresh-generated-V3-contracts-a"
  :intent "wo-20260531101448-Refresh-generated-V3-contracts-a"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531101448-Refresh-generated-V3-contracts-a-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check"
                    "git diff --check"])))
