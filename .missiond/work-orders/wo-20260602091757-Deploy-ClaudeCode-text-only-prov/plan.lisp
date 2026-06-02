(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602091757-Deploy-ClaudeCode-text-only-prov"
  :intent "wo-20260602091757-Deploy-ClaudeCode-text-only-prov"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602091757-Deploy-ClaudeCode-text-only-prov-shard-default"
       :read_scope ["."]
       :write_scope
         [".missiond/v3/shards/workstation-runtime.lisp"
          "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
          "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
          "crates/missiond-daemon/src/handlers/comm/conversation/query.rs"
          "crates/missiond-daemon/src/handlers/knowledge/intent.rs"
          "crates/missiond-daemon/src/provider_box/claude_code_driver.rs"
          "crates/missiond-daemon/src/provider_box/http_adapter.rs"
          "crates/missiond-daemon/src/provider_box/types.rs"
          "crates/missiond-pty/src/session.rs"
          "scripts/generated/v3_contracts.d.ts"
          "scripts/generated/v3_contracts.mjs"
          "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance
         ["node scripts/check-v3-interactive-provider-box.mjs --json"
          "node scripts/project-v3-contracts.mjs --check --json"
          "cargo test -p missiond-daemon provider_box::claude_code_driver::tests -- --nocapture"
          "cargo test -p missiond-daemon provider_box::http_adapter::tests -- --nocapture"
          "git diff --check"])))
