(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260523-v3-ssot-architecture-convergence"
  :intent "20260523-v3-ssot-architecture-convergence"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260523-v3-ssot-architecture-convergence-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/**"
                     ".missiond/work-orders/20260523-v3-ssot-architecture-convergence/**"
                     "crates/missiond-core/**"
                     "crates/missiond-daemon/**"
                     "scripts/**"
                     "tools/missiond_lispc/**"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-typed-lisp-compiler.mjs"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-semantic-checker-coverage.mjs --json"
                    "cargo test -p missiond-daemon v3_blueprint_runtime"
                    "cargo test -p missiond-daemon file_artifacts"
                    "cargo test -p missiond-daemon request::"
                    "cargo test -p missiond-core event"
                    "cargo test --workspace"
                    "pnpm --dir packages/board build"
                    "git diff --check"])))
