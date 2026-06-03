(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603062334-Add-ClaudeCode-deep-research-pro"
  :intent "wo-20260603062334-Add-ClaudeCode-deep-research-pro"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603062334-Add-ClaudeCode-deep-research-pro-shard-default"
       :read_scope ["."]
       :write_scope
         ["crates/missiond-daemon/src/provider_box/http_adapter.rs"
          "crates/missiond-daemon/src/provider_box/claude_code_driver.rs"]
       :acceptance
         ["cargo test -p missiond-daemon claude_code_workflow -- --nocapture"
          "cargo test -p missiond-daemon claude_code_deep_research -- --nocapture"
          "cargo test -p missiond-daemon codex_sources_export_live_smoke_routeability -- --nocapture"
          "git diff --check"])))
