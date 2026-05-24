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
       :approval-mode plan
       :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
       :runtime-rule "Gemini handles investigation/review/context-pack until scoped-write smoke passes; runtime must spawn it with --approval-mode plan --policy .missiond/v3/policies/gemini-readonly-policy.toml, never --yolo, and the policy denies subagents/write/shell tools.")
     (gemini-fast-survey
       :account current-gemini-cli-login
       :model "gemini-2.5-flash"
       :write-policy read-only
       :authority low
       :approval-mode plan
       :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
       :runtime-rule "Only mechanical scan/summary work; never architecture裁决; runtime must spawn it with --approval-mode plan --policy .missiond/v3/policies/gemini-readonly-policy.toml, never --yolo.")
     (agy-research
       :account current-antigravity-login
       :model "Antigravity default"
       :write-policy read-only
       :authority medium
       :runtime-rule "Agy is the Gemini successor research lane. MissionD launches `agy --prompt-interactive` as a PTY, recognizes idle/running/blocked/unavailable screens, reads post-claim markdown artifacts from `$HOME/.gemini/antigravity-cli/brain` or `MISSIOND_AGY_ARTIFACT_ROOT` as provider durable finals, and keeps it read-only until provider regression smoke passes.")
     (codex-code-worker
       :account current-codex-cli-login
       :model "gpt-5.5"
       :reasoning-effort xhigh
       :search true
       :sandbox danger-full-access
       :approval-policy never
       :write-policy scoped-shard
       :runtime-rule "Ordinary Codex implementation lane. It is separate from codex-master-control and may only consume accepted exact shards with write_scope/lease/artifact requirements. Runtime MUST launch Codex with --cd <slot project root> in addition to PTY cwd so resumed/profile state cannot drift to a stale repository.")
     (codex-review-worker
       :account current-codex-cli-login
       :model "gpt-5.5"
       :reasoning-effort xhigh
       :search true
       :sandbox read-only
       :approval-policy never
       :write-policy read-only
       :runtime-rule "Codex review/regression lane. It can audit designs and code but cannot replace resident master-control. Runtime MUST launch Codex with --cd <slot project root> in addition to PTY cwd so review evidence and slot metadata share the same repository root.")
     (claude-code-fast-patch
       :account current-claude-code-default
       :model "Sonnet only when explicitly selected"
       :write-policy narrow-scoped
       :runtime-rule "Consumes pre-atomized exact file/region patch tasks, not open-ended coding.")
     (claude-code-deploy-ops
       :account current-claude-code-default
       :model "Claude Code Default (user-selected Opus 4.7/1M)"
       :write-policy observe-and-plan
       :runtime-rule "Consumes context_intent=deploy-ops / task_class=deploy-ops BoardTasks. Query deploy-center/provenance/events first, produce rollback/redeploy plans and smoke evidence, and never mutate production/DNS/secrets unless deploy-center policy or explicit Board approval is present.")
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
     "mission_slots exposes ClaudeCode/Gemini/Codex/Agy provider, modelProfile, reasoningEffort, PTY recognition, latestConversation, and mismatch diagnostics."
     "Autopilot logs selected V3 workstation-pool slot before claim."])
  :next-account-isolation
    ["Add auth/env profile fields after single-login smoke is stable."
     "Split claude-code-default into claude-max-a and claude-max-b with disjoint env/profile roots."])
