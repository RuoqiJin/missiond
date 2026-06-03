(audit
  :schema "missiond.work-order-audit.v1"
  :authority "codex-cli-gpt-5.5-xhigh"
  :work-order-id "wo-20260603071432-Fix-Codex-provider-box-durable-f"
  :finding "Jarvis plan authoring failed because Codex provider-box treated a transient input-idle screen as terminal durable-final-missing before the rollout JSONL final was written."
  :root-cause ["Codex provider-box monitor used a hard-coded 3 second idle grace after the PTY returned to input." "Codex rollout task_complete/final_answer can lag the input-idle screen by more than 3 seconds." "Jarvis plan failure handling wrote SSE diagnostics but did not reuse the durable replay diagnostic path."]
  :fix ["Added a bounded configurable Codex durable final idle grace, defaulting to 45 seconds." "Included idle_grace_secs in durable-final-missing diagnostics." "Routed Jarvis plan authoring failures through fail_jarvis_gate_visible so replay gets a diagnostic event." "Added and ran focused unit coverage plus provider-box regression tests."]
  :remaining-risk ["The public Jarvis endpoint still has an apparent 30 second edge timeout for long exact workflows; after deploy, verify whether durable replay/follow produces the user-visible reply or whether an additional async/streaming edge change is required."])
