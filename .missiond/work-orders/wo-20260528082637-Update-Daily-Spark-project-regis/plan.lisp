(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528082637-Update-Daily-Spark-project-regis"
  :intent "wo-20260528082637-Update-Daily-Spark-project-regis"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528082637-Update-Daily-Spark-project-regis-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/universe/project-maturity.lisp"
                     ".missiond/v3/shards/universe/project-registry.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260528082637-Update-Daily-Spark-project-regis/intent.lisp"
                     ".missiond/work-orders/wo-20260528082637-Update-Daily-Spark-project-regis/plan.lisp"
                     ".missiond/work-orders/wo-20260528082637-Update-Daily-Spark-project-regis/audit.lisp"]
       :acceptance ["node scripts/check-project-ssot-universe.mjs --json"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
