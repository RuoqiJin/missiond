(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528072730-Emit-visible-Jarvis-gate-progres"
  :intent "wo-20260528072730-Emit-visible-Jarvis-gate-progres"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528072730-Emit-visible-Jarvis-gate-progres-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
                     ".missiond/work-orders/wo-20260528072730-Emit-visible-Jarvis-gate-progres"]
       :acceptance ["rustfmt --edition 2021 --check crates/missiond-core/src/ws/server.rs"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "git diff --check"])))
