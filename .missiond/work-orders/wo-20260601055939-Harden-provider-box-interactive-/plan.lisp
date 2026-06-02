(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601055939-Harden-provider-box-interactive-"
  :intent "wo-20260601055939-Harden-provider-box-interactive-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601055939-Harden-provider-box-interactive--shard-default"
       :read_scope ["."]
       :write_scope
         [".missiond/v3/shards/request-runtime.lisp"
          ".missiond/v3/shards/workstation-runtime.lisp"
          ".missiond/work-orders/wo-20260601055939-Harden-provider-box-interactive-"
          "crates/missiond-daemon/src/context"
          "crates/missiond-daemon/src/handlers/compute/pty.rs"
          "crates/missiond-daemon/src/provider_box"
          "crates/missiond-pty/src"
          "packages/board/src/components/PtyTeachingPanel.tsx"
          "scripts/check-v3-interactive-provider-box.mjs"
          "scripts/generated"]
       :acceptance
         ["cargo check -p missiond-daemon"
          "cargo test -p missiond-daemon codex_exec -- --nocapture"
          "cargo test -p missiond-daemon text_only -- --nocapture"
          "node scripts/project-v3-contracts.mjs --check --json"
          "node scripts/check-v3-interactive-provider-box.mjs --json"])))
