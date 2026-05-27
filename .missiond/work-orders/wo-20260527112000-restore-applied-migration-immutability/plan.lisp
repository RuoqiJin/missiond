(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527112000-restore-applied-migration-immutability"
  :intent "wo-20260527112000-restore-applied-migration-immutability"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527112000-restore-applied-migration-immutability-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/migrations/20260527000000_control_plane_kernel.sql"
                     "crates/missiond-core/migrations/20260527002000_capability_grants_spawn_operation.sql"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     "scripts/check-pg-migrations-discipline.mjs"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-pg-migrations-discipline.mjs"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
