(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260604181332-Project-deploy-service-manifests"
  :intent "wo-20260604181332-Project-deploy-service-manifests"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260604181332-Project-deploy-service-manifests-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260604181332-Project-deploy-service-manifests/**"
                     ".missiond/v3/shards/universe/service-runtime.lisp"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/check-v3-runtime-domain-projections.mjs"
                    "node scripts/check-deployment-channel-coverage.mjs"
                    "node scripts/check-domain-proxy-isomorphism.mjs"
                    "node scripts/check-project-ssot-universe.mjs --json"])))
