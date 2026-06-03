(intent-draft
  :schema "missiond.work-order-intent.v1"
  :authority "codex-cli-gpt-5.5-xhigh"
  :channel "codex-goal"
  :original-message "Run the xiaojinpro iOS Jarvis backend prompt through the real backend channel, fix failures, and obtain a usable reply."
  :objective "Fix Jarvis grounded_direct_answer routing after plan_archived so Codex direct-answer requests do not target an AGY communicator slot and failures remain visible in durable replay."
  :intent-kind "implementation"
  :confidence "high"
  :understanding "The backend workflow reached plan_archived but failed to complete direct answer because the configured Codex direct-answer provider inherited an AGY communicator slot/model default. The failure also did not persist as an interaction diagnostic after the public SSE client had disconnected."
  :non-goals ["Do not add deterministic Rust answer fallback." "Do not bypass provider-box." "Do not change the iOS client."]
  :acceptance-signals ["Codex direct-answer override uses a Codex provider-box slot and gpt-5.5 model." "AGY communicator default uses a private provider-box text-only slot." "Visible gate failures persist diagnostic interaction events."])
