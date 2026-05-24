(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260524-launchd-runtime-root-deploy"
  :intent "20260524-launchd-runtime-root-deploy"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260524-launchd-runtime-root-deploy-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/ops-surfaces.lisp"
                     ".missiond/v3/shards/ops-infra.lisp"
                     "scripts/deploy-daemon.sh"
                     "scripts/check-missiond-blue-green-deploy.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"]
       :acceptance ["bash -n scripts/deploy-daemon.sh"
                    "node scripts/check-missiond-blue-green-deploy.mjs --json"
                    "node scripts/check-v3-ops-infra-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
