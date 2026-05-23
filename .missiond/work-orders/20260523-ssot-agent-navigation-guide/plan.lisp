(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260523-ssot-agent-navigation-guide"
  :intent "20260523-ssot-agent-navigation-guide"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260523-ssot-agent-navigation-guide-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/**"
                     ".missiond/work-orders/20260523-ssot-agent-navigation-guide/**"
                     "crates/missiond-daemon/**"
                     "crates/missiond-mcp/**"
                     "scripts/**"
                     "tools/missiond_lispc/**"]
       :acceptance ["node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-agent-entry-slices.mjs --json"
                    "node scripts/check-v3-capability-governance-isomorphism.mjs --json"
                    "node scripts/check-v3-semantic-checker-coverage.mjs --json"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/check-typed-lisp-compiler.mjs"
                    "cargo test -p missiond-daemon tool_directory"
                    "cargo test -p missiond-daemon context_slice_agent_entry_selects_by_intent"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
