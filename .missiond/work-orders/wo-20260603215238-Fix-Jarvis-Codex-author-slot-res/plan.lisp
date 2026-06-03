(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603215238-Fix-Jarvis-Codex-author-slot-res"
  :intent "wo-20260603215238-Fix-Jarvis-Codex-author-slot-res"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603215238-Fix-Jarvis-Codex-author-slot-res-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/compile-v3-runtime.mjs --json"
                    "node scripts/project-v3-contracts.mjs --write"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-jarvis-runtime-topology.mjs --json"
                    "cargo fmt --all --check"
                    "cargo check -p missiond-core -p missiond-daemon"
                    "cargo test -p missiond-core jarvis -- --nocapture"])))
