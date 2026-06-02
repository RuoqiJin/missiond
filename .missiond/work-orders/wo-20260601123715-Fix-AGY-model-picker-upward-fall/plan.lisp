(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601123715-Fix-AGY-model-picker-upward-fall"
  :intent "wo-20260601123715-Fix-AGY-model-picker-upward-fall"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601123715-Fix-AGY-model-picker-upward-fall-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/agy_driver.rs"]
       :acceptance ["cargo test -p missiond-daemon provider_box::agy_driver::tests::model_picker_bounded_scan_uses_up_after_down_fallback -- --nocapture"
                    "cargo test -p missiond-daemon provider_box::agy_driver::tests::model_picker_navigation_plan_counts_visible_down_steps -- --nocapture"
                    "git diff --check"])))
