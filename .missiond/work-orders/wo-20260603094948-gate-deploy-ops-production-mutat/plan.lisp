(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603094948-gate-deploy-ops-production-mutat"
  :intent "wo-20260603094948-gate-deploy-ops-production-mutat"
  :status active
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603094948-gate-deploy-ops-production-mutat-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "crates/missiond-mcp/src/tools/compute/task_delegate.rs"
                     "scripts/check-v3-deployment-closure-plane.mjs"]
       :acceptance ["cargo fmt --package missiond-daemon --package missiond-mcp --check"
                    "node scripts/check-v3-deployment-closure-plane.mjs --json"
                    "git diff --check"
                    "cargo test -p missiond-mcp task_delegate --lib"
                    "cargo test -p missiond-daemon deploy_ops"])))
