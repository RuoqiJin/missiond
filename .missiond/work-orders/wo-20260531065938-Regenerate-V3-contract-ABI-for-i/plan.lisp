(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531065938-Regenerate-V3-contract-ABI-for-i"
  :intent "wo-20260531065938-Regenerate-V3-contract-ABI-for-i"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531065938-Regenerate-V3-contract-ABI-for-i-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260531065938-Regenerate-V3-contract-ABI-for-i/intent.lisp"
                     ".missiond/work-orders/wo-20260531065938-Regenerate-V3-contract-ABI-for-i/plan.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-typed-lisp-compiler.mjs --json"
                    "git diff --check"])))
