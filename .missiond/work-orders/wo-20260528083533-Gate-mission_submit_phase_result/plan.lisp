(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528083533-Gate-mission_submit_phase_result"
  :intent "wo-20260528083533-Gate-mission_submit_phase_result"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528083533-Gate-mission_submit_phase_result-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/feature_gates.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528083533-Gate-mission_submit_phase_result/intent.lisp"
                     ".missiond/work-orders/wo-20260528083533-Gate-mission_submit_phase_result/plan.lisp"
                     ".missiond/work-orders/wo-20260528083533-Gate-mission_submit_phase_result/audit.lisp"]
       :acceptance ["cargo test -p missiond-daemon feature_gates::tests::non_core_tools_are_feature_gated"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
