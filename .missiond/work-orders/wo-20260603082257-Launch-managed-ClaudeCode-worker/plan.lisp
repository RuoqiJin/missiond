(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603082257-Launch-managed-ClaudeCode-worker"
  :intent "wo-20260603082257-Launch-managed-ClaudeCode-worker"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603082257-Launch-managed-ClaudeCode-worker-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"
                     "crates/missiond-daemon/src/slot_orchestrator/generic_cli.rs"
                     "scripts/check-v3-workstation-pool-isomorphism.mjs"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --write"
                    "cargo fmt --check"
                    "cargo test -p missiond-daemon missiond_managed_claude_code_slots_default_to_bypass -- --nocapture"
                    "cargo test -p missiond-daemon provider_box -- --nocapture"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"])))
