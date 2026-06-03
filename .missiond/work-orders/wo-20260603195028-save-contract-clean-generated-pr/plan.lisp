(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603195028-save-contract-clean-generated-pr"
  :intent "wo-20260603195028-save-contract-clean-generated-pr"
  :status active
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603195028-save-contract-clean-generated-pr-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["git diff --check"])))
