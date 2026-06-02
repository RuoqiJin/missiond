(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-context-gather-ranked-skill-facts"
  :intent "wo-context-gather-ranked-skill-facts"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-context-gather-ranked-skill-facts-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-context-gather-ranked-skill-facts/**"]
       :acceptance ["node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "cargo test -p missiond-daemon --bin missiond context_gather::tests"
                    "live mission_context_gather(source_profile=deploy_ops) returns query-ranked skill operational facts"])))
