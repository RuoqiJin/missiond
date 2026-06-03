(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603-deployment-closure-plane"
  :intent "wo-20260603-deployment-closure-plane"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603-deployment-closure-plane-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/templates/independent-app-bootstrap/"
                     ".missiond/v3/"
                     ".missiond/workflows/"
                     "crates/missiond-core/src/"
                     "crates/missiond-daemon/src/"
                     "docs/guides/"
                     "scripts/"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-deployment-closure-plane.mjs --json"
                    "node scripts/check-v3-production-runtime-boundary.mjs --json"
                    "node scripts/scaffold-product-deployment-closure.mjs --self-test"
                    "cargo check -p missiond-core -p missiond-daemon"])))
