(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260516-skill-sync-watcher"
  :intent "20260516-skill-sync-watcher"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260516-skill-sync-watcher-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/db/pg/skill.rs"
                     "crates/missiond-core/src/db/traits.rs"
                     "crates/missiond-core/src/skill.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/skill/query.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/state.rs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
