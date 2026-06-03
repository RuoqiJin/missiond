(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-infra-deploy-closure-evidence-lane"
  :intent "wo-infra-deploy-closure-evidence-lane"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-infra-deploy-closure-evidence-lane-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
                     ".missiond/v3/shards/ops-infra.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "scripts/check-v3-ops-infra-isomorphism.mjs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-infra-deploy-closure-evidence-lane/**"]
       :acceptance ["cargo test -p missiond-daemon --bin missiond sysinfra::infra::tests"
                    "node scripts/check-v3-ops-infra-isomorphism.mjs --json"
                    "node scripts/check-v3-memory-kb-isomorphism.mjs --json"])))
