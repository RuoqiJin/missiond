(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601190606-Harden-provider-box-interactive-"
  :intent "wo-20260601190606-Harden-provider-box-interactive-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601190606-Harden-provider-box-interactive--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/universe/service-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/provider_box/agy_driver.rs"
                     "crates/missiond-daemon/src/provider_box/claude_code_driver.rs"
                     "crates/missiond-daemon/src/provider_box/codex_driver.rs"
                     "crates/missiond-daemon/src/provider_box/http_adapter.rs"
                     "crates/missiond-daemon/src/provider_box/types.rs"
                     "crates/missiond-pty/src/pty_recognition.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon provider_box::agy_driver::tests -- --nocapture"
                    "cargo test -p missiond-daemon provider_box::http_adapter::tests -- --nocapture"
                    "cargo test -p missiond-daemon provider_box::codex_driver::tests -- --nocapture"
                    "cargo test -p missiond-daemon provider_box::claude_code_driver::tests -- --nocapture"
                    "cargo test -p missiond-pty claude_code -- --nocapture"
                    "cargo check -p missiond-daemon"
                    "cargo check -p missiond-pty"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-runtime-domain-projections.mjs --json"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
                    "git diff --check"])))
