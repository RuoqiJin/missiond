(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528080108-Update-project-universe-registry"
  :intent "wo-20260528080108-Update-project-universe-registry"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528080108-Update-project-universe-registry-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/universe/project-maturity.lisp"
                     ".missiond/v3/shards/universe/project-registry.lisp"
                     ".missiond/v3/shards/universe/service-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "scripts/check-project-ssot-universe.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/lib/behavior_universe.mjs"
                     ".missiond/work-orders/wo-20260528080108-Update-project-universe-registry"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-project-ssot-universe.mjs --json"
                    "rustfmt --edition 2021 --check crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                    "git diff --check"])))
