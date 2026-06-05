(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260605164101-Codify-Deploy-Center-migration-a"
  :intent "wo-20260605164101-Codify-Deploy-Center-migration-a"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260605164101-Codify-Deploy-Center-migration-a-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/deployment-closure-plane.lisp"
                     "scripts/compile-v3-runtime.mjs"
                     "scripts/check-v3-deployment-closure-plane.mjs"
                     "scripts/check-deployment-channel-coverage.mjs"]
       :acceptance ["node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-deployment-closure-plane.mjs --json"
                    "node scripts/check-deployment-channel-coverage.mjs --json"])))
