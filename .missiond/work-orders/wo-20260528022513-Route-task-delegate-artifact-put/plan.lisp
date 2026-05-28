(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528022513-Route-task-delegate-artifact-put"
  :intent "wo-20260528022513-Route-task-delegate-artifact-put"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528022513-Route-task-delegate-artifact-put-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
