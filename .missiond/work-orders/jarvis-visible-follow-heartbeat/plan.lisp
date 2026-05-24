(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "jarvis-visible-follow-heartbeat"
  :intent "jarvis-visible-follow-heartbeat"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "jarvis-visible-follow-heartbeat-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-core jarvis_visible_heartbeat_budget_is_bounded -- --nocapture"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
