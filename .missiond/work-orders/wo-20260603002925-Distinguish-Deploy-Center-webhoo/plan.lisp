(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603002925-Distinguish-Deploy-Center-webhoo"
  :intent "wo-20260603002925-Distinguish-Deploy-Center-webhoo"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603002925-Distinguish-Deploy-Center-webhoo-shard-default"
       :read_scope [".missiond/v3/missiond-blueprint.lisp"
                    ".missiond/v3/shards/index.lisp"
                    ".missiond/v3/shards/request-runtime.lisp"
                    ".missiond/v3/shards/universe/service-runtime.lisp"
                    "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                    "services/deploy-center/src/workers/deploy_event_relay.rs"]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/work-orders/wo-20260603002925-Distinguish-Deploy-Center-webhoo"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --write"
                    "node scripts/compile-v3-runtime.mjs --json"
                    "node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "cargo test -p missiond-daemon --bin missiond deployment_event_ -- --nocapture"
                    "cargo check -p missiond-daemon"])))
