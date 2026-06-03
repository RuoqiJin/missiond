(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603011103-Surface-deployment-event-evidenc"
  :intent "wo-20260603011103-Surface-deployment-event-evidenc"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603011103-Surface-deployment-event-evidenc-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "scripts/project-v3-contracts.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
