(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603195256-merge-local-temporary-branches-i"
  :intent "wo-20260603195256-merge-local-temporary-branches-i"
  :status active
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603195256-merge-local-temporary-branches-i-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["git diff --check"
                    "git status --short --branch"])))
