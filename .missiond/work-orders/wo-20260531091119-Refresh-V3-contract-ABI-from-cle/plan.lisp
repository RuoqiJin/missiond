(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531091119-Refresh-V3-contract-ABI-from-cle"
  :intent "wo-20260531091119-Refresh-V3-contract-ABI-from-cle"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531091119-Refresh-V3-contract-ABI-from-cle-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260531091119-Refresh-V3-contract-ABI-from-cle/intent.lisp"
                     ".missiond/work-orders/wo-20260531091119-Refresh-V3-contract-ABI-from-cle/plan.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["clean worktree: node scripts/project-v3-contracts.mjs --check --json"
                    "git diff --check"])))
