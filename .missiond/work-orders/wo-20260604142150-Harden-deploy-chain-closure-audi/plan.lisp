(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260604142150-Harden-deploy-chain-closure-audi"
  :intent "wo-20260604142150-Harden-deploy-chain-closure-audi"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260604142150-Harden-deploy-chain-closure-audi-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260604142150-Harden-deploy-chain-closure-audi/**"
                     ".missiond/v3/shards/deployment-closure-plane.lisp"
                     ".missiond/v3/shards/index.lisp"
                     ".missiond/v3/shards/universe/service-runtime.lisp"
                     ".missiond/v3/runtime/compiled/**"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/audit-deploy-chain-closure.mjs"
                     "scripts/check-v3-deployment-closure-plane.mjs"
                     "scripts/compile-v3-runtime.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-deployment-closure-plane.mjs --json"
                    "node scripts/check-v3-runtime-domain-projections.mjs --json"
                    "node scripts/audit-deploy-chain-closure.mjs --json --include-conversation-audit"])))
