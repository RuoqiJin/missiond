(audit
  :schema "missiond.work-order-audit.v1"
  :authority "codex-cli-gpt-5.5-xhigh"
  :work-order-id "wo-20260603064251-Fix-Jarvis-direct-answer-slot"
  :finding "Jarvis direct answer had provider/slot/model coupling drift: MISSIOND_JARVIS_DIRECT_ANSWER_PROVIDER=codex_cli still inherited the AGY communicator slot default and communicator model fallback."
  :root-cause ["direct-answer slot selection reused MISSIOND_JARVIS_COMMUNICATOR_SLOT_ID even when the direct-answer provider was explicitly Codex." "direct-answer model selection fell back to the communicator model instead of a provider-specific Codex model." "failure diagnostics were written to SSE but not persisted as interaction events, so a disconnected public client could leave replay stuck at plan_archived."]
  :fix ["Added provider-specific direct-answer slot/model helpers." "Changed AGY text-only default slot to slot-agy-gemini-31-pro-high." "Persisted fail_jarvis_gate_visible diagnostic events to interaction replay." "Added focused unit coverage."]
  :remaining-risk ["Public /jarvis/v1/chat/completions still appears to have a hard edge timeout; durable replay/follow endpoints must be used for long workflows unless the edge is changed to async polling or longer streaming."])
