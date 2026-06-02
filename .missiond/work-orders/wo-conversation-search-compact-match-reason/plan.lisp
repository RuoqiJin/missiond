(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-conversation-search-compact-match-reason"
  :intent "wo-conversation-search-compact-match-reason"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-conversation-search-compact-match-reason-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/comm/conversation/query.rs"
                     "crates/missiond-mcp/src/tools/comm/conversation.rs"]
       :acceptance ["cargo fmt --all --check"
                    "cargo test -p missiond-daemon --bin missiond conversation::query::tests"])))
