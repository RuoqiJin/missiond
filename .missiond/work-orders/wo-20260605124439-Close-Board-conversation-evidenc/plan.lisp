(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260605124439-Close-Board-conversation-evidenc"
  :intent "wo-20260605124439-Close-Board-conversation-evidenc"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260605124439-Close-Board-conversation-evidenc-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/universe/behavior-closure.lisp"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-daemon/src/provider_box/claude_code_driver.rs"
                     "packages/board/src/app/api/conversation-evidence/route.ts"
                     "packages/board/src/app/api/xjpcode/latency/route.ts"
                     "packages/board/src/components/Conversations.tsx"
                     "packages/board/src/components/XjpcodePanel.tsx"
                     "scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/lib/behavior_universe.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
