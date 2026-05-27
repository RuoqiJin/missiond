(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527110000-deploy-socket-readiness"
  :intent "wo-20260527110000-deploy-socket-readiness"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527110000-deploy-socket-readiness-shard-default"
       :read_scope ["."]
       :write_scope ["scripts/deploy-daemon.sh"
                     "scripts/check-v3-ops-infra-isomorphism.mjs"
                     ".missiond/v3/shards/ops-infra.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["bash -n scripts/deploy-daemon.sh"
                    "node scripts/check-v3-ops-infra-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
