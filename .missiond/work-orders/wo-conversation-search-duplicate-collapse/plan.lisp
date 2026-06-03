(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-conversation-search-duplicate-collapse"
  :intent "wo-conversation-search-duplicate-collapse"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-conversation-search-duplicate-collapse-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/memory-knowledge-runtime.lisp"
                     ".missiond/v3/shards/implementation/runtime-surfaces.lisp"
                     "scripts/check-v3-conversation-ingestion-isomorphism.mjs"
                     "crates/missiond-daemon/src/handlers/comm/conversation/query.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-mcp/src/tools/comm/conversation.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo fmt --all --check"
                    "cargo test -p missiond-daemon --bin missiond conversation::query::tests"
                    "cargo test -p missiond-daemon --bin missiond context_gather::tests"
                    "node scripts/check-v3-conversation-ingestion-isomorphism.mjs --json"
                    "node scripts/project-v3-contracts.mjs --check --json"])))
