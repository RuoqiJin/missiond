(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260525-jarvis-runtime-chain"
  :intent "20260525-jarvis-runtime-chain"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260525-jarvis-runtime-chain-shard-default"
       :read_scope ["."]
       :write_scope [".githooks/**"
                     ".missiond/frontend/**"
                     ".missiond/v3/shards/**"
                     ".missiond/workflows/**"
                     "crates/missiond-core/**"
                     "crates/missiond-daemon/src/context/**"
                     "packages/board/**"
                     "scripts/**"]
       :acceptance ["node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-work-order-lifecycle-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "pnpm --dir packages/board build"
                    "cargo check -p missiond-core"
                    "bash scripts/rustfmt-missiond.sh --check"
                    "git diff --check"])))
