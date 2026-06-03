(plan-draft
  :schema "missiond.work-order-plan.v1"
  :authority "codex-cli-gpt-5.5-xhigh"
  :objective "Repair Jarvis direct-answer provider-box routing and durable diagnostics."
  :execution-mode "implementation"
  :requires-board-task false
  :steps [
    (step s1 :status completed :text "Reproduce provider-box error for codex_cli direct-answer request sent to slot-agy-gemini-communicator.")
    (step s2 :status completed :text "Make Jarvis direct-answer slot/model selection provider-specific; explicit Codex override defaults to slot-codex-provider-box and gpt-5.5.")
    (step s3 :status completed :text "Make AGY text-only defaults use the hidden private Gemini 3.1 Pro high slot instead of slot-agy-research.")
    (step s4 :status completed :text "Persist fail_jarvis_gate_visible diagnostics to the interaction replay ledger.")
    (step s5 :status completed :text "Add unit tests for Codex direct-answer override and AGY communicator defaults.")
    (step s6 :status pending :text "Deploy to mac mini and rerun the real Jarvis prompt through the public backend channel until a durable final reply exists.")
  ]
  :write-scope ["crates/missiond-core/src/ws/server.rs"]
  :verification ["cargo fmt --check" "cargo test -p missiond-core jarvis_direct_answer_codex_override_uses_codex_slot_and_model -- --nocapture" "cargo test -p missiond-core jarvis_communicator_agy_defaults_to_private_text_slot -- --nocapture" "cargo test -p missiond-core jarvis_sse_disconnect_errors_are_non_terminal -- --nocapture" "cargo test -p missiond-daemon provider_box -- --nocapture"])
