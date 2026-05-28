(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528063239-Wire-Jarvis-plan-author-through-"
  :intent "wo-20260528063239-Wire-Jarvis-plan-author-through-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528063239-Wire-Jarvis-plan-author-through--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-core/src/ws/mod.rs"
                     "crates/missiond-core/src/lib.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
                     ".missiond/v3/evidence/workstation-pool.lisp"
                     ".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/check-v3-workstation-pool-isomorphism.mjs"
                     "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260528063239-Wire-Jarvis-plan-author-through-"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-daemon"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"
                    "node scripts/check-v3-behavior-closure.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
