(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260524-jarvis-public-prefix-demux"
  :intent "20260524-jarvis-public-prefix-demux"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260524-jarvis-public-prefix-demux-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/work-orders/20260524-jarvis-public-prefix-demux/**"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-behavior-closure.mjs --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo test -p missiond-core public_jarvis_prefix_normalizes_to_daemon_routes -- --nocapture"
                    "cargo test -p missiond-core openai_chat_request_normalizes_to_interaction_envelope -- --nocapture"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
