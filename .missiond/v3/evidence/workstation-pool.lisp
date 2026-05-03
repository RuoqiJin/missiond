(workstation-pool-evidence
  :schema "missiond.v3.evidence.workstation-pool.v1"
  :owner workstation-pool
  :purpose "Keep V3 core Lisp compact while preserving implementation evidence for workstation pool routing."
  :single-login-phase
    ((claude-code-default
       :account current-claude-code-default
       :model "Claude Code Default (user-selected Opus 4.7/1M)"
       :runtime-rule "omit --model for coding-default-opus-4-7")
     (gemini-ultra
       :account current-gemini-cli-login
       :write-policy read-only
       :runtime-rule "Compatibility alias for the current slot-gemini-ultra PTY.")
     (gemini-ultra-pro
       :account current-gemini-cli-login
       :model "gemini-3.1-pro-preview"
       :write-policy read-only
       :runtime-rule "Gemini handles investigation/review/context-pack until scoped-write smoke passes")
     (gemini-fast-survey
       :account current-gemini-cli-login
       :model "gemini-2.5-flash"
       :write-policy read-only
       :authority low
       :runtime-rule "Only mechanical scan/summary work; never architecture裁决.")
     (claude-code-fast-patch
       :account current-claude-code-default
       :model "Sonnet only when explicitly selected"
       :write-policy narrow-scoped
       :runtime-rule "Consumes pre-atomized exact file/region patch tasks, not open-ended coding.")
     (codex-master-control
       :account current-codex-cli-login
       :model "gpt-5.5"
       :reasoning-effort xhigh
       :search true
       :sandbox danger-full-access
       :approval-policy never
       :write-policy audited-full-access
       :runtime-rule "Resident brain/orchestrator; has full local sandbox access, reads Lisp/Board/KB/events, dispatches workers, checkpoints decisions, and must leave Board/KB/checkpoint evidence for any direct mutation."))
  :observability
    ["mission_compute_slot action=list exposes workstation_pool."
     "Each pool row reports runtime_slot_present and status."
     "mission_slots exposes ClaudeCode/Gemini/Codex provider, modelProfile, reasoningEffort, PTY recognition, latestConversation, and mismatch diagnostics."
     "Autopilot logs selected V3 workstation-pool slot before claim."])
  :next-account-isolation
    ["Add auth/env profile fields after single-login smoke is stable."
     "Split claude-code-default into claude-max-a and claude-max-b with disjoint env/profile roots."])
