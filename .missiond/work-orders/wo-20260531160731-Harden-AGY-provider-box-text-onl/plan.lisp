(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531160731-Harden-AGY-provider-box-text-onl"
  :intent "wo-20260531160731-Harden-AGY-provider-box-text-onl"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531160731-Harden-AGY-provider-box-text-onl-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/provider_box/agy_driver.rs"
                     "crates/missiond-daemon/src/provider_box/http_adapter.rs"
                     "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
                     "crates/missiond-pty/src/manager.rs"
                     "crates/missiond-pty/src/pty_recognition.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260531160731-Harden-AGY-provider-box-text-onl"]
       :acceptance ["cargo fmt --check --package missiond-daemon --package missiond-pty"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
