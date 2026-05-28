(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528084500-Board-api-structured-errors"
  :intent "wo-20260528084500-Board-api-structured-errors"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528084500-Board-api-structured-errors-shard-default"
       :read_scope ["."]
       :write_scope ["packages/board/src/app/api/tasks/route.ts"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "pnpm --dir packages/board build"
                    "git diff --check"])))
