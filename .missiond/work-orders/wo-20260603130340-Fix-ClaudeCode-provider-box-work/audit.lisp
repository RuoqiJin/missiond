(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260603130340-Fix-ClaudeCode-provider-box-work"
  :events ((event created :at "2026-06-03T13:03:40.939Z" :actor missiond-work-order)
           (event implemented :at "2026-06-03T13:05:00Z" :actor codex
             :summary "Provider-box ClaudeCode worker-turn now treats a stable output_contract.must_write_file artifact as canonical completion, preserving durable final priority and avoiding PTY screen fallback when Jarvis grounding report is already written.")))
