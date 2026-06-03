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
                     "crates/missiond-core/src/ws/server.rs"]
       :acceptance ["node scripts/compile-v3-runtime.mjs --json"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-jarvis-runtime-topology.mjs --json"
                    "cargo fmt --all --check"
                    "cargo check -p missiond-core -p missiond-daemon"
                    "cargo test -p missiond-core jarvis -- --nocapture"])))
