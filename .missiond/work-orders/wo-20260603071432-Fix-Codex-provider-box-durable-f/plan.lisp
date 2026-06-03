(plan-draft
  :schema "missiond.work-order-plan.v1"
  :authority "codex-cli-gpt-5.5-xhigh"
  :objective "Fix Codex provider-box durable final settle and Jarvis replay diagnostics for plan authoring."
  :execution-mode "implementation"
  :requires-board-task false
  :steps [
    (step s1 :status completed :text "Reproduce the Jarvis public backend failure: plan authoring failed with PROVIDER_DURABLE_FINAL_MISSING even though Codex wrote a valid durable final shortly afterward.")
    (step s2 :status completed :text "Increase Codex provider-box durable final idle grace from a hard-coded 3 seconds to a bounded configurable settle window.")
    (step s3 :status completed :text "Expose idle_grace_secs in the durable-final-missing diagnostic for future operator debugging.")
    (step s4 :status completed :text "Route Jarvis plan authoring failures through fail_jarvis_gate_visible so diagnostic events persist to interaction replay.")
    (step s5 :status completed :text "Harden Codex prompt submission so large bracketed pastes wait for screen confirmation and provider-box requires correlated rollout acknowledgement.")
    (step s6 :status completed :text "Reduce Jarvis plan author prompt size while preserving grounding report file/hash and key judgment evidence.")
    (step s7 :status completed :text "Add focused unit coverage and rerun provider-box tests.")
    (step s8 :status pending :text "Deploy to mac mini and rerun the same iOS backend-channel prompt until a durable reply is produced.")
  ]
  :write-scope ["crates/missiond-daemon/src/provider_box/codex_driver.rs" "crates/missiond-core/src/ws/server.rs" "crates/missiond-pty/src/session.rs"]
  :verification ["cargo fmt --check" "cargo test -p missiond-daemon codex_durable_final_idle_grace_is_bounded -- --nocapture" "cargo test -p missiond-daemon rollout_ack_detects_correlated_user_message_before_final -- --nocapture" "cargo test -p missiond-core jarvis_plan_author_timeout_budget_is_bounded -- --nocapture" "cargo test -p missiond-core jarvis_direct_answer_codex_override_uses_codex_slot_and_model -- --nocapture" "cargo test -p missiond-daemon provider_box -- --nocapture"])
