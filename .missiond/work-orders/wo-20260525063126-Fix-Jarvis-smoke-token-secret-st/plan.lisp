(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260525063126-Fix-Jarvis-smoke-token-secret-st"
  :intent "wo-20260525063126-Fix-Jarvis-smoke-token-secret-st"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260525063126-Fix-Jarvis-smoke-token-secret-st-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/pillar-flow-map.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/smoke-jarvis-interaction.mjs"]
       :acceptance ["node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/smoke-jarvis-interaction.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
