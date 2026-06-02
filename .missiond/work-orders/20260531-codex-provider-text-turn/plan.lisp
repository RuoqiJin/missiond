(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260531-codex-provider-text-turn"
  :intent "20260531-codex-provider-text-turn"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260531-codex-provider-text-turn-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/codex_driver.rs"
                     "crates/missiond-core/src/agy_cli/parser.rs"
                     "crates/missiond-core/src/agy_cli/watcher.rs"
                     "crates/missiond-core/src/ws/server.rs"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/check-v3-interactive-provider-box.mjs"
                     "scripts/deploy-daemon.sh"
                     ".missiond/work-orders/20260531-codex-provider-text-turn/*"]
       :acceptance ["cargo test -p missiond-daemon provider_box::codex_driver"
                    "bash -n scripts/deploy-daemon.sh"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "git diff --check"])))
