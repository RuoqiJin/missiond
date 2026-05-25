(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260525-self-update-contract-hygiene"
  :intent "wo-20260525-self-update-contract-hygiene"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260525-self-update-contract-hygiene-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/ops-surfaces.lisp"
                     ".missiond/workflows/missiond-macmini-self-update.lisp"
                     "scripts/deploy-daemon.sh"
                     "scripts/check-missiond-blue-green-deploy.mjs"
                     "scripts/check-v3-macmini-self-update-lane.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/check-missiond-blue-green-deploy.mjs --json"
                    "node scripts/check-v3-macmini-self-update-lane.mjs --json"
                    "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "bash -n scripts/deploy-daemon.sh"
                    "git diff --check"])))
