(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531133910-Add-guarded-provider-box-PTY-obs"
  :intent "wo-20260531133910-Add-guarded-provider-box-PTY-obs"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531133910-Add-guarded-provider-box-PTY-obs-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/work-orders/wo-20260531133910-Add-guarded-provider-box-PTY-obs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/provider_box"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "cargo fmt --check"
                    "cargo test -p missiond-daemon provider_box -- --nocapture"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "git diff --check"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only ; attempted, currently blocked by unrelated repository-wide blueprint/project maturity/behavior-closure failures"])))
