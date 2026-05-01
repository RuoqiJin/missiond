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
       :runtime-rule "Gemini handles investigation/review/context-pack until scoped-write smoke passes"))
  :observability
    ["mission_compute_slot action=list exposes workstation_pool."
     "Each pool row reports runtime_slot_present and status."
     "Autopilot logs selected V3 workstation-pool slot before claim."])
  :next-account-isolation
    ["Add auth/env profile fields after single-login smoke is stable."
     "Split claude-code-default into claude-max-a and claude-max-b with disjoint env/profile roots."])
