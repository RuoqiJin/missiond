(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601182636-Harden-provider-box-router-lanes"
  :intent "wo-20260601182636-Harden-provider-box-router-lanes"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601182636-Harden-provider-box-router-lanes-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["cargo test -p missiond-daemon provider_box::http_adapter::tests -- --nocapture"
                    "cargo test -p missiond-daemon provider_box::agy_driver::tests -- --nocapture"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-runtime-domain-projections.mjs --json"
                    "git diff --check"])))
