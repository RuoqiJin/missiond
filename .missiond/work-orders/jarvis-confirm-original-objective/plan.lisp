(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "jarvis-confirm-original-objective"
  :intent "jarvis-confirm-original-objective"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "jarvis-confirm-original-objective-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/jarvis-confirm-original-objective/**"]
       :acceptance ["cargo test -p missiond-core jarvis_confirmation -- --nocapture"
                   "cargo test -p missiond-core interaction_confirmation -- --nocapture"
                   "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                   "node scripts/check-v3-final-convergence.mjs --json --static-only"
                   "git diff --check"])))
