(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531122557-Consolidate-provider-box-archite"
  :intent "wo-20260531122557-Consolidate-provider-box-archite"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531122557-Consolidate-provider-box-archite-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/**"
                     "CLAUDE.md"
                     "crates/**"
                     "missiond-core/**"
                     "missiond-daemon/**"
                     "missiond-mcp/**"
                     "scripts/**"
                     "tools/**"]
       :acceptance ["cargo fmt --check"
                    "cargo check -p missiond-daemon"
                    "cargo test -p missiond-daemon provider_box -- --nocapture"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-workstation-config-isomorphism.mjs --json"
                    "node scripts/check-v3-workstation-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-xjpcode-portable-runtime.mjs --json"
                    "node scripts/check-v3-project-registry-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"])))
