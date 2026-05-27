(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527055224-Retry-transient-Jarvis-follow-tr"
  :intent "wo-20260527055224-Retry-transient-Jarvis-follow-tr"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527055224-Retry-transient-Jarvis-follow-tr-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/universe/behavior-closure.lisp"
                     ".missiond/work-orders/wo-20260527055224-Retry-transient-Jarvis-follow-tr/**"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/smoke-jarvis-intent-plan-dispatch.mjs"]
       :acceptance ["node --check scripts/smoke-jarvis-intent-plan-dispatch.mjs"
                   "node scripts/check-v3-behavior-closure.mjs --json"
                   "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                   "node scripts/check-v3-final-convergence.mjs --json --static-only"
                   "git diff --check"])))
