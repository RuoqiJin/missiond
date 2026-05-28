(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528084453-Add-control-plane-kernel-schema-"
  :intent "wo-20260528084453-Add-control-plane-kernel-schema-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528084453-Add-control-plane-kernel-schema--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/tests/pg_integration.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528084453-Add-control-plane-kernel-schema-/intent.lisp"
                     ".missiond/work-orders/wo-20260528084453-Add-control-plane-kernel-schema-/plan.lisp"
                     ".missiond/work-orders/wo-20260528084453-Add-control-plane-kernel-schema-/audit.lisp"]
       :acceptance ["cargo test -p missiond-core --test pg_integration --features postgres --no-run"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
