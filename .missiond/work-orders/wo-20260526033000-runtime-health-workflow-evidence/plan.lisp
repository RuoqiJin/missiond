(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526033000-runtime-health-workflow-evidence"
  :intent "wo-20260526033000-runtime-health-workflow-evidence"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526033000-runtime-health-workflow-evidence-shard-default"
       :read_scope ["."]
       :write_scope ["Cargo.toml"
                     "Cargo.lock"
                     "crates/missiond-core/**"
                     "crates/missiond-daemon/**"
                     "crates/missiond-domain/**"
                     "packages/board/src/**"
                     "scripts/check-*.mjs"
                     "scripts/deploy-daemon.sh"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-daemon -p missiond-mcp"
                    "pnpm --dir packages/board build"
                    "node scripts/check-high-roi-contracts.mjs --json"
                    "node scripts/check-v3-architecture-boundaries.mjs --json"
                    "node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                    "node scripts/check-board-operator-fixtures.mjs"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
