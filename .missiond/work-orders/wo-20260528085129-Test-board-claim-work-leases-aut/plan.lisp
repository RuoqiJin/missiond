(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528085129-Test-board-claim-work-leases-aut"
  :intent "wo-20260528085129-Test-board-claim-work-leases-aut"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528085129-Test-board-claim-work-leases-aut-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/tests/pg_integration.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528085129-Test-board-claim-work-leases-aut/intent.lisp"
                     ".missiond/work-orders/wo-20260528085129-Test-board-claim-work-leases-aut/plan.lisp"
                     ".missiond/work-orders/wo-20260528085129-Test-board-claim-work-leases-aut/audit.lisp"]
       :acceptance ["cargo test -p missiond-core --test pg_integration --features postgres --no-run"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
