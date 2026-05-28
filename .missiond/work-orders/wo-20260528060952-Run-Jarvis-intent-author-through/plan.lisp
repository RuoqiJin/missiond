(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528060952-Run-Jarvis-intent-author-through"
  :intent "wo-20260528060952-Run-Jarvis-intent-author-through"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528060952-Run-Jarvis-intent-author-through-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     ".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     ".missiond/v3/evidence/workstation-pool.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/lib/behavior_universe.mjs"
                     "scripts/propose-behavior-navigation.mjs"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "cargo test -p missiond-core ws::server::tests -- --nocapture"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"
                    "node scripts/check-v3-behavior-closure.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
