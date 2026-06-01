(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601151357-Add-Codex-permissions-provider-b"
  :intent "Add Codex provider-box APIs for switching /permissions modes through observe-act-observe PTY control."
  :status active
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601151357-Add-Codex-permissions-provider-b-shard-default"
       :read_scope ["."]
       :write_scope
         [".missiond/v3/shards/workstation-runtime.lisp"
          "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
          "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
          "crates/missiond-daemon/src/provider_box/agy_driver.rs"
          "crates/missiond-daemon/src/provider_box/codex_driver.rs"
          "crates/missiond-daemon/src/provider_box/http_adapter.rs"
          "crates/missiond-daemon/src/provider_box/types.rs"
          "crates/missiond-pty/src/pty_recognition.rs"
          "scripts/generated/v3_contracts.d.ts"
          "scripts/generated/v3_contracts.mjs"
          "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance
         ["cargo test -p missiond-pty codex_permission_picker -- --nocapture"
          "cargo test -p missiond-daemon codex_permission_mode_aliases_normalize -- --nocapture"
          "cargo test -p missiond-daemon slot_permission_mode_suffixes_map_to_codex_permission_modes -- --nocapture"
          "node scripts/check-v3-interactive-provider-box.mjs --json"])))
