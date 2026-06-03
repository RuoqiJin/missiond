(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603195107-save-release-snapshot-deployment"
  :intent "wo-20260603195107-save-release-snapshot-deployment"
  :status active
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603195107-save-release-snapshot-deployment-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/deployment-closure-plane.lisp"
                     ".missiond/v3/shards/implementation/ops-surfaces.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-missiond-blue-green-deploy.mjs"
                     "scripts/deploy-daemon.sh"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["git diff --check"])))
