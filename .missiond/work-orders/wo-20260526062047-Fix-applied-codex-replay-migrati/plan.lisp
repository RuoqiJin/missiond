(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526062047-Fix-applied-codex-replay-migrati"
  :intent "wo-20260526062047-Fix-applied-codex-replay-migrati"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526062047-Fix-applied-codex-replay-migrati-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/migrations/20260523003000_codex_replay.sql"
                     "scripts/check-pg-migrations-discipline.mjs"]
       :acceptance ["node scripts/check-pg-migrations-discipline.mjs --json"
                   "shasum -a 384 crates/missiond-core/migrations/20260523003000_codex_replay.sql"
                   "git diff --check"])))
