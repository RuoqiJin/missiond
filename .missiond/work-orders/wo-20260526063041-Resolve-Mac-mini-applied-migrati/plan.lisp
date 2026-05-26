(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526063041-Resolve-Mac-mini-applied-migrati"
  :intent "wo-20260526063041-Resolve-Mac-mini-applied-migrati"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526063041-Resolve-Mac-mini-applied-migrati-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/migrations/20260526000000_conversation_session_management.sql"
                     "crates/missiond-core/migrations/20260526001000_worker_runtime_state.sql"
                     "crates/missiond-core/migrations/20260526003000_worker_runtime_state.sql"
                     "scripts/check-high-roi-contracts.mjs"
                     "scripts/check-pg-migrations-discipline.mjs"]
       :acceptance ["node scripts/check-pg-migrations-discipline.mjs --json"
                   "node scripts/check-high-roi-contracts.mjs"
                   "cargo check -p missiond-core"
                   "git diff --check"])))
