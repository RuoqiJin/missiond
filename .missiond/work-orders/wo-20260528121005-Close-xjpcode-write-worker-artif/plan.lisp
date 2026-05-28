(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528121005-Close-xjpcode-write-worker-artif"
  :intent "wo-20260528121005-Close-xjpcode-write-worker-artif"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528121005-Close-xjpcode-write-worker-artif-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     ".missiond/work-orders/wo-20260528121005-Close-xjpcode-write-worker-artif/audit.lisp"
                     ".missiond/work-orders/wo-20260528121005-Close-xjpcode-write-worker-artif/intent.lisp"
                     ".missiond/work-orders/wo-20260528121005-Close-xjpcode-write-worker-artif/plan.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "scripts/check-v3-xjpcode-portable-runtime.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
