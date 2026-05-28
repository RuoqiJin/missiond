(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528064739-Consume-exact-settle-capability-"
  :intent "wo-20260528064739-Consume-exact-settle-capability-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528064739-Consume-exact-settle-capability--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/shared_memory.rs"
                     "crates/missiond-core/src/ws/server.rs"
                     ".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260528064739-Consume-exact-settle-capability-"
                     ".missiond/work-orders/wo-20260528070300-Reuse-Jarvis-confirmed-artifacts"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-daemon"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
