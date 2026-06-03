(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-conversation-type-filter-plumbing"
  :intent "wo-conversation-type-filter-plumbing"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-conversation-type-filter-plumbing-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/comm/conversation"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-mcp/src/tools/comm/conversation.rs"
                     "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "scripts/check-v3-conversation-ingestion-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "node scripts/check-v3-conversation-ingestion-isomorphism.mjs --json"
                    "cargo test -p missiond-daemon --bin missiond context_gather::tests"
                    "cargo test -p missiond-daemon --bin missiond conversation::query::tests"])))
