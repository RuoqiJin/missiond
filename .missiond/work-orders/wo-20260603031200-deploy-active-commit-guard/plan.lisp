(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603031200-deploy-active-commit-guard"
  :intent "wo-20260603031200-deploy-active-commit-guard"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603031200-deploy-active-commit-guard-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/ops-infra.lisp"
                     ".missiond/v3/shards/deployment-closure-plane.lisp"
                     ".missiond/v3/shards/implementation/ops-surfaces.lisp"
                     "scripts/deploy-daemon.sh"
                     "scripts/check-missiond-blue-green-deploy.mjs"
                     "scripts/check-v3-ops-infra-isomorphism.mjs"
                     "scripts/check-v3-deployment-closure-plane.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["bash -n scripts/deploy-daemon.sh"
                    "node scripts/check-missiond-blue-green-deploy.mjs --json"
                    "node scripts/check-v3-ops-infra-isomorphism.mjs"
                    "node scripts/check-v3-deployment-closure-plane.mjs"
                    "node scripts/project-v3-contracts.mjs --check --json"])))
