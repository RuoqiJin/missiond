(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260523-jarvis-context-pack-file"
  :intent "20260523-jarvis-context-pack-file"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260523-jarvis-context-pack-file-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/work-orders/20260523-jarvis-context-pack-file/intent.lisp"
                     ".missiond/work-orders/20260523-jarvis-context-pack-file/plan.lisp"
                     ".missiond/work-orders/20260523-jarvis-context-pack-file/audit.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-core jarvis_"
                    "bash scripts/rustfmt-missiond.sh --check"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
