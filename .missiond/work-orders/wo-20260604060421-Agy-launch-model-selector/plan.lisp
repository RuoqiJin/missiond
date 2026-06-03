(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260604060421-Agy-launch-model-selector"
  :intent "wo-20260604060421-Agy-launch-model-selector"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260604060421-Agy-launch-model-selector-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     ".missiond/work-orders/wo-20260604060421-Agy-launch-model-selector/intent.lisp"
                     ".missiond/work-orders/wo-20260604060421-Agy-launch-model-selector/plan.lisp"
                     ".missiond/work-orders/wo-20260604060421-Agy-launch-model-selector/audit.lisp"
                     ".missiond/work-orders/wo-20260603215238-Fix-Jarvis-Codex-author-slot-res/plan.lisp"
                     "crates/missiond-pty/src/session.rs"
                     "crates/missiond-pty/src/pty_recognition.rs"
                     "crates/missiond-daemon/src/provider_box/agy_driver.rs"
                     "crates/missiond-daemon/src/provider_box/http_adapter.rs"
                     "scripts/check-v3-interactive-provider-box.mjs"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["agy --help"
                    "agy models"
                    "node scripts/compile-v3-runtime.mjs --json"
                    "node scripts/project-v3-contracts.mjs --write"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "cargo fmt --all --check"
                    "cargo check -p missiond-pty -p missiond-daemon"
                    "cargo test -p missiond-pty agy_command_uses_interactive_tui_with_help_confirmed_launch_toggles -- --nocapture"
                    "cargo test -p missiond-daemon agy_spawn_options_project_provider_box_bypass_and_model_launch -- --nocapture"
                    "cargo test -p missiond-daemon agy_slot_capabilities_expose_bypass_and_launch_model_selector -- --nocapture"])))
