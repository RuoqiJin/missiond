(intent
  :id "20260524-jarvis-worker-context-slice"
  :schema "missiond.work-order.intent.v1"
  :summary "Make Jarvis worker prompts self-contained for external CLI workers after intent/plan confirmation."
  :problem "Codex ordinary worker validation succeeded but reported that the materialized grounding context pack does not contain the confirmed plan body or target lane details. Workers without MissionD MCP should not have to infer these from broad context."
  :desired-outcome "Jarvis dispatch prompts include target lane metadata plus confirmed intent/plan artifact references and a compact accepted execution slice so read-only workers can execute without rediscovery."
  :constraints [ssot-first no-fallback no-rsync touched-rustfmt-only]
  :acceptance ["cargo test -p missiond-core jarvis_worker_prompt_prefers_materialized_context_file"
               "node scripts/check-v3-final-convergence.mjs --json --static-only"
               "git diff --check"])
