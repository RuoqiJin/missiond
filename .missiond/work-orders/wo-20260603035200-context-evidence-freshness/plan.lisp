(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603035200-context-evidence-freshness"
  :intent "wo-20260603035200-context-evidence-freshness"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603035200-context-evidence-freshness-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "cargo test -p missiond-daemon context_gather --lib"])))
