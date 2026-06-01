(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601095310-Add-provider-box-CLI-task-source"
  :intent "wo-20260601095310-Add-provider-box-CLI-task-source"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601095310-Add-provider-box-CLI-task-source-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/**" ".antigravitycli/**" "crates/**" "packages/**" "scripts/**" "*.md"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
