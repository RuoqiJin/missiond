(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260604052536-Record-xjp-invoice-service-runti"
  :intent "wo-20260604052536-Record-xjp-invoice-service-runti"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260604052536-Record-xjp-invoice-service-runti-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/universe/service-runtime.lisp"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --write"
                    "node scripts/compile-v3-runtime.mjs --write --json"
                    "node scripts/check-project-ssot-universe.mjs --json"])))
