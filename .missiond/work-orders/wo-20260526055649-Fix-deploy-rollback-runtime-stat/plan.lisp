(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526055649-Fix-deploy-rollback-runtime-stat"
  :intent "wo-20260526055649-Fix-deploy-rollback-runtime-stat"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526055649-Fix-deploy-rollback-runtime-stat-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/ops-infra.lisp"
                     ".missiond/work-orders/wo-20260526055649-Fix-deploy-rollback-runtime-stat/audit.lisp"
                     ".missiond/work-orders/wo-20260526055649-Fix-deploy-rollback-runtime-stat/intent.lisp"
                     ".missiond/work-orders/wo-20260526055649-Fix-deploy-rollback-runtime-stat/plan.lisp"
                     "crates/missiond-core/migrations/20260318000000_init.sql"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-high-roi-contracts.mjs"
                     "scripts/check-pg-migrations-discipline.mjs"
                     "scripts/check-v3-ops-infra-isomorphism.mjs"
                     "scripts/deploy-daemon.sh"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-pg-migrations-discipline.mjs --json"
                    "node scripts/check-v3-ops-infra-isomorphism.mjs --json"
                    "node scripts/check-high-roi-contracts.mjs --json"
                    "bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-core -p missiond-daemon"
                    "git diff --check"])))
