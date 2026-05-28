(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528045046-Harden-interaction-observability"
  :intent "wo-20260528045046-Harden-interaction-observability"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528045046-Harden-interaction-observability-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-core/src/db/pg/observability.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/check-v3-plan-execution-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
                     "scripts/smoke-jarvis-interaction.mjs"]
       :acceptance ["cargo check -p missiond-daemon"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
