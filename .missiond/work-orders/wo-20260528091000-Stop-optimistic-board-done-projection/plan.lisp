(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528091000-Stop-optimistic-board-done-projection"
  :intent "wo-20260528091000-Stop-optimistic-board-done-projection"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528091000-Stop-optimistic-board-done-projection-shard-default"
       :read_scope ["."]
       :write_scope ["packages/board/src/store.ts"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "pnpm --dir packages/board build"
                    "git diff --check"])))
