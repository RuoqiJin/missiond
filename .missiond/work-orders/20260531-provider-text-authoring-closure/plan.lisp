(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260531-provider-text-authoring-closure"
  :intent "20260531-provider-text-authoring-closure"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260531-provider-text-authoring-closure-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/provider_box/agy_driver.rs"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/smoke-jarvis-interaction.mjs"
                     ".missiond/work-orders/20260531-provider-text-authoring-closure/*"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-core -p missiond-daemon"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "git diff --check"])))
