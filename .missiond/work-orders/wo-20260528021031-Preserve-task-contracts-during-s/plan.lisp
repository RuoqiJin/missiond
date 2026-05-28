(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528021031-Preserve-task-contracts-during-s"
  :intent "wo-20260528021031-Preserve-task-contracts-during-s"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528021031-Preserve-task-contracts-during-s-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
