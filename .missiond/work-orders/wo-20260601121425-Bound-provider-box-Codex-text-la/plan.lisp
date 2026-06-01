(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601121425-Bound-provider-box-Codex-text-la"
  :intent "wo-20260601121425-Bound-provider-box-Codex-text-la"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601121425-Bound-provider-box-Codex-text-la-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/provider_box/codex_driver.rs"
                     "crates/missiond-daemon/src/provider_box/http_adapter.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon provider_box::codex_driver::tests -- --nocapture"
                    "cargo test -p missiond-daemon provider_box::http_adapter::tests -- --nocapture"
                    "node scripts/check-v3-interactive-provider-box.mjs"
                    "node scripts/check-v3-runtime-domain-projections.mjs"
                    "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
                    "git diff --check"])))
