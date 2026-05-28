(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528095524-Bind-learning-and-retrospective-"
  :intent "wo-20260528095524-Bind-learning-and-retrospective-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528095524-Bind-learning-and-retrospective--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
                     "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
                     "crates/missiond-daemon/src/workers/local/experience_harvester.rs"
                     "crates/missiond-daemon/src/workers/sonnet/retro_worker.rs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "scripts/check-v3-conversation-ingestion-isomorphism.mjs"]
       :acceptance ["cargo fmt --check -p missiond-daemon"
                    "node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "node scripts/check-v3-conversation-ingestion-isomorphism.mjs --json"])))
