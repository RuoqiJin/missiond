(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260523-agent-navigation-closure"
  :intent "20260523-agent-navigation-closure"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260523-agent-navigation-closure-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/**"
                     ".missiond/frontend/**"
                     ".missiond/work-orders/20260523-agent-navigation-closure/**"
                     "crates/missiond-daemon/**"
                     "crates/missiond-mcp/**"
                     "packages/board/**"
                     "scripts/**"
                     "tools/missiond_lispc/**"]
       :acceptance ["node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-agent-entry-slices.mjs --json"
                    "node scripts/check-v3-agent-navigation-quality.mjs --json --check"
                    "node scripts/check-v3-agent-navigation-closure.mjs --json"
                    "node scripts/check-v3-capability-governance-isomorphism.mjs --json"
                    "node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                    "node scripts/check-v3-semantic-checker-coverage.mjs --json"
                    "node scripts/check-typed-lisp-compiler.mjs"
                    "node scripts/project-frontend-board-config.mjs --check"
                    "node scripts/check-frontend-board-lisp-schema.mjs"
                    "node scripts/check-frontend-board-code-isomorphism.mjs"
                    "node scripts/check-frontend-board-runtime-projection.mjs"
                    "pnpm --dir packages/board build"
                    "cargo test -p missiond-daemon tool_directory"
                    "cargo test -p missiond-daemon agent_navigation"
                    "cargo test -p missiond-daemon context_slice"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
