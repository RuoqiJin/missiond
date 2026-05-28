(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528080402-Commit-incubating-universe-gate-"
  :intent "wo-20260528080402-Commit-incubating-universe-gate-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528080402-Commit-incubating-universe-gate--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/universe/project-maturity.lisp"
                     ".missiond/v3/shards/universe/project-registry.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260528080402-Commit-incubating-universe-gate-"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-project-ssot-universe.mjs --json"
                    "git diff --check"])))
