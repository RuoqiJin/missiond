(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260604-project-universe-read-model"
  :intent "wo-20260604-project-universe-read-model"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260604-project-universe-read-model-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/universe/project-maturity.lisp"
                     ".missiond/v3/shards/universe/project-registry.lisp"
                     ".missiond/v3/shards/universe/service-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/project/reconcile.rs"
                     "scripts/check-deployment-channel-coverage.mjs"
                     "scripts/check-project-ssot-universe.mjs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/check-v3-project-registry-isomorphism.mjs"
                     "scripts/check-v3-workstation-pool-isomorphism.mjs"
                     "scripts/compile-v3-runtime.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260604-project-universe-read-model"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-project-registry-isomorphism.mjs --json"
                    "node scripts/check-project-ssot-universe.mjs --json"
                    "node scripts/check-deployment-channel-coverage.mjs --json"
                    "node scripts/check-project-maturity.mjs --min-level M5 --json"
                    "cargo check -p missiond-daemon"])))
