(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531-agy-ingestion-ack-safe"
  :intent "wo-20260531-agy-ingestion-ack-safe"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531-agy-ingestion-ack-safe-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/agy_cli/watcher.rs"
                     "crates/missiond-core/src/db/pg/infra.rs"
                     "crates/missiond-daemon/src/infra/message_handler.rs"
                     "crates/missiond-daemon/src/workers/local/conversation_logger.rs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
