(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602181820-Route-MissionD-authority-evidenc"
  :intent "wo-20260602181820-Route-MissionD-authority-evidenc"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602181820-Route-MissionD-authority-evidenc-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/memory-knowledge-runtime.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
