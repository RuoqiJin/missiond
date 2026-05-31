(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531063642-Interaction-ledger-durable-repla"
  :intent "wo-20260531063642-Interaction-ledger-durable-repla"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531063642-Interaction-ledger-durable-repla-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/work-orders/wo-20260531063642-Interaction-ledger-durable-repla/intent.lisp"
                     ".missiond/work-orders/wo-20260531063642-Interaction-ledger-durable-repla/plan.lisp"
                     ".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/index.lisp"
                     ".missiond/v3/shards/memory-knowledge-runtime.lisp"
                     ".missiond/v3/shards/pillar-flow-map.lisp"
                     ".missiond/v3/shards/v2-convergence-map.lisp"
                     "crates/missiond-core/src/db/pg/conversation.rs"
                     "crates/missiond-core/src/db/traits.rs"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/handlers/comm/interaction.rs"
                     "scripts/check-v3-interaction-ledger-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-interaction-ledger-isomorphism.mjs --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-typed-lisp-compiler.mjs --json"
                    "cargo check -p missiond-daemon"
                    "rustfmt --edition 2021 --check crates/missiond-daemon/src/handlers/comm/interaction.rs"
                    "git diff --check"])))
