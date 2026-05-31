(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531085640-Regenerate-V3-contract-ABI-after"
  :intent "wo-20260531085640-Regenerate-V3-contract-ABI-after"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531085640-Regenerate-V3-contract-ABI-after-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260531085640-Regenerate-V3-contract-ABI-after/intent.lisp"
                     ".missiond/work-orders/wo-20260531085640-Regenerate-V3-contract-ABI-after/plan.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/check-typed-lisp-compiler.mjs --json"
                    "git diff --check"])))
