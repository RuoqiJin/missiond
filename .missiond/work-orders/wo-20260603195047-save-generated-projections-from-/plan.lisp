(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603195047-save-generated-projections-from-"
  :intent "wo-20260603195047-save-generated-projections-from-"
  :status active
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603195047-save-generated-projections-from--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["git diff --check"])))
