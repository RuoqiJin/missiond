(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602205213-context-gather-diagnostics-tightening"
  :intent "wo-20260602205213-context-gather-diagnostics-tightening"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602205213-context-gather-diagnostics-tightening-shard-default"
       :read_scope ["."]
       :write_scope
         [".missiond/v3/shards/request-runtime.lisp"
          ".missiond/v3/shards/universe/infrastructure.lisp"
          ".missiond/v3/shards/universe/service-runtime.lisp"
          "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
          "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
          "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
          "scripts/check-v3-eventbridge-isomorphism.mjs"
          "scripts/check-v3-memory-kb-isomorphism.mjs"
          "scripts/generated/v3_contracts.d.ts"
          "scripts/generated/v3_contracts.mjs"
          "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance
         ["node scripts/check-v3-memory-kb-isomorphism.mjs --json"
          "node scripts/check-v3-memory-kb-isomorphism.mjs --dry-fixture --json"
          "node scripts/check-v3-eventbridge-isomorphism.mjs --json"
          "node scripts/project-v3-contracts.mjs --check --json"
          "cargo test -p missiond-daemon --bin missiond context_gather"
          "cargo check -p missiond-daemon"
          "git diff --check"])))
