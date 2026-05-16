(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260516-codex-boot-work-order-gate"
  :intent "20260516-codex-boot-work-order-gate"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260516-codex-boot-work-order-gate-shard-default"
       :read_scope ["."]
       :write_scope [".githooks/pre-commit"
                     ".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/evidence/codex-boot-context.lisp"
                     ".missiond/workflows/work-order-lifecycle.lisp"
                     ".missiond/work-orders/20260516-codex-boot-work-order-gate"
                     "crates/missiond-daemon/src/handlers/mod.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
                     "scripts/missiond-work-order.mjs"
                     "scripts/hooks/pre-commit-missiond-work-order"
                     "scripts/check-v3-codex-boot-context-isomorphism.mjs"
                     "scripts/check-v3-work-order-lifecycle-isomorphism.mjs"
                     "scripts/check-v3-source-hygiene-isomorphism.mjs"
                     "scripts/check-v3-code-isomorphism-complete.mjs"]
       :acceptance ["node scripts/check-v3-source-hygiene-isomorphism.mjs --json"
                    "node scripts/check-v3-work-order-lifecycle-isomorphism.mjs --json"
                    "node scripts/check-v3-codex-boot-context-isomorphism.mjs --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo check -p missiond-mcp -p missiond-daemon"
                    "git diff --check"])))
