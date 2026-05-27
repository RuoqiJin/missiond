(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527095752-Converge-control-plane-runtime-a"
  :intent "wo-20260527095752-Converge-control-plane-runtime-a"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527095752-Converge-control-plane-runtime-a-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo test --workspace"
                    "pnpm --dir packages/board build"
                    "git diff --check"])))
