(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527102834-Fix-capability-grant-operation-c"
  :intent "wo-20260527102834-Fix-capability-grant-operation-c"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527102834-Fix-capability-grant-operation-c-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/migrations/20260527000000_control_plane_kernel.sql"
                     "crates/missiond-core/migrations/20260527002000_capability_grants_spawn_operation.sql"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["scripts/rustfmt-missiond.sh --check"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo test -p missiond-daemon readonly_result_task -- --nocapture"
                    "cargo test -p missiond-daemon task_result_artifact -- --nocapture"
                    "git diff --check"])))
