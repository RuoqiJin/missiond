(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603044705-Fix-stale-conversation-timestamp"
  :intent "wo-20260603044705-Fix-stale-conversation-timestamp"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603044705-Fix-stale-conversation-timestamp-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/db/pg/conversation.rs"
                     "crates/missiond-daemon/src/provider_box/claude_code_driver.rs"
                     "crates/missiond-daemon/src/provider_box/codex_driver.rs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
