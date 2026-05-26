(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526021418-Remove-MissionD-owned-SQLite-run"
  :intent "wo-20260526021418-Remove-MissionD-owned-SQLite-run"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526021418-Remove-MissionD-owned-SQLite-run-shard-default"
       :read_scope ["."]
       :write_scope [".github/workflows/quality-gates.yml"
                     ".missiond/v3/shards"
                     "Cargo.toml"
                     "Cargo.lock"
                     "README.md"
                     "clippy.toml"
                     "crates"
                     "docs"
                     "packages"
                     "scripts"]
       :acceptance ["cargo check -p missiond-core -p missiond-daemon -p missiond-mcp -p skill-store"
                   "scripts/cargo-fmt-touched.sh --check"
                   "pnpm --dir packages/board typecheck"
                   "pnpm --dir packages/board build"
                   "node scripts/check-v3-ops-infra-isomorphism.mjs --json"
                   "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs --json"
                   "node scripts/check-missiond-owned-sqlite-clean.mjs --json"
                   "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                   "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                   "git diff --check"])))
