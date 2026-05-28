  (workstation-config
    :desc "Lisp-owned workstation spawn policy; runtime slot config is a projection, not an independent default."
    :config-fields [:template :model_profile :model :cwd :project_root :mcp_config :ttl :permission_mode]
    :resolution-order [caller-model caller-model-profile task-intent-template claude-code-default]
    (model-profile coding-default-opus-4-7
      :applies-to [code research]
      :claude-code-ui "Default recommended"
      :effective-model "Opus 4.7 with 1M context"
      :spawn-model-arg nil
      :rule "Omit --model so Claude Code uses the user's Default model selection.")
    (model-profile research-default
      :applies-to [research review context-pack lisp-compression]
      :pool-binding gemini-ultra-pro
      :spawn-model-arg nil
      :rule "Research-class delegations route to the workstation-pool gemini researcher slot. spawn-model-arg is nil because the binding is to a pre-spawned Gemini PTY; explicit caller model_profile=coding-default-opus-4-7 overrides this and pins the work to Claude.")
    (model-profile gemini-ultra-pro-preview
      :applies-to [research review context-pack lisp-compression language-explanation]
      :pool-binding gemini-ultra-pro
      :spawn-model-arg "gemini-3.1-pro-preview"
      :rule "Gemini Ultra defaults to Gemini 3.1 Pro Preview for high-level read-only investigation; lower-authority fast survey lanes must be explicitly requested.")
    (model-profile codex-master-gpt-5-5-xhigh
      :applies-to [master-control orchestration governance night-audit]
      :pool-binding codex-master-control
      :spawn-model-arg "gpt-5.5"
      :reasoning-effort xhigh
      :search true
      :sandbox workspace-write
      :approval-policy never
      :rule "Resident master control uses Codex GPT-5.5 with xhigh reasoning and full local sandbox access. It remains an audited orchestrator: every direct mutation must leave Board/KB/checkpoint evidence, while ordinary implementation still prefers delegated workers.")
    (model-profile daily-sonnet
      :applies-to [ops low-risk-maintenance]
      :spawn-model-arg "sonnet"
      :rule "Use only when the task or caller explicitly asks for Sonnet-class daily work.")
    (model-profile quick-haiku
      :applies-to [docs test chore low-risk-fast-path]
      :spawn-model-arg "haiku"
      :rule "Use only when the task or caller explicitly asks for a low-cost fast Claude Code model.")
    (slot-template coder
      :role coder
      :description "Dynamic coder slot (ephemeral)"
      :default-model-profile coding-default-opus-4-7
      :mcp-config "$MISSION_HOME/xjp-mcp-config.json"
      :default-cwd "$MISSIOND_PROJECTS_DIR")
    (slot-template researcher
      :role coder
      :description "Dynamic researcher slot (read-only analysis)"
      :default-model-profile research-default
      :mcp-config "$MISSION_HOME/xjp-mcp-config.json"
      :default-cwd "$MISSIOND_PROJECTS_DIR")
    (slot-template ops
      :role operator
      :description "Dynamic ops slot (ephemeral)"
      :default-model-profile daily-sonnet
      :mcp-config "$MISSION_HOME/xjp-mcp-config.json"
      :default-cwd "$MISSIOND_PROJECTS_DIR")
    (cwd-policy dynamic-slot
      :allowed-prefixes ["$MISSIOND_PROJECTS_DIR" "$MISSIOND_DOWNLOADS_DIR" "$MISSIOND_DOCUMENTS_DIR" "/tmp"])
    (chat-completions-policy jarvis-api
      :default_slot "slot-claude-code-default"
      :header_override "X-Slot-Id"
      :auto_heal_env MISSIOND_JARVIS_SLOT_AUTO_HEAL
      :auto_heal_timeout_env MISSIOND_JARVIS_SLOT_AUTO_HEAL_TIMEOUT_SECS
      :rule "OpenAI-compatible /v1/chat/completions routes to the explicit X-Slot-Id header when present; otherwise it uses this V3-projected default slot. Rust must not hardcode slot-jarvis. Jarvis broad requests MUST pass the strict intent/plan gate: emit intent_draft and await user confirmation, then emit plan_draft and await confirmation, then create BoardTask metadata with grounding_context_id, intent_artifact_id, and plan_artifact_id before any worker dispatch. The OpenAI-compatible adapter MUST mirror intent_draft and plan_draft as missiond.openai-artifact-projection.v1 delta chunks with artifact_id/artifact_path/summary so clients that only render chat deltas still show reviewable artifacts. Intent/plan draft events MUST expose review_text plus artifact_body/artifact_language on the semantic event itself; a client confirmation UI is invalid if the user cannot inspect the concrete artifact body before confirming. The gate MUST persist confirm_required payloads as missiond.jarvis-pending-confirmation.v1 conversation markers and resolve conservative text-only confirmations from that marker before using the latest user text as a new objective. Jarvis plan-confirmed dispatch classifies the user objective verb into review/design/implementation/verification via classify_jarvis_dispatch_verb: implementation verbs (implement, fix, create, build, add, refactor, migrate, 补齐, 接入, 实现, 修复, 重构, 提交, 推送) produce task_class=code with non-empty write_scope; when the objective explicitly asks for Codex, dispatch MUST use engine_hint=codex and pool_hint=codex-code-worker, otherwise scoped code work may use engine_hint=claude_code and pool_hint=claude-code-default. review/smoke/read-only verbs remain task_class=review, write_policy=read-only with empty write_scope. /jarvis/api/monitor/jarvis is read-only and may report auto_heal policy; /internal/jarvis/slot/ensure is a localhost-only deploy smoke control surface for restoring the default Exited/Error slot after blue-green restart. Chat requests, local ensure, and deploy-daemon post-switch smoke may attempt at most one restart of an Exited/Error default slot when MISSIOND_JARVIS_SLOT_AUTO_HEAL=1; failure returns JARVIS_SLOT_UNAVAILABLE or JARVIS_SLOT_AUTO_HEAL_FAILED typed diagnostic and never falls back to direct local code search or fake output. MissionD deploys MUST run the local ensure endpoint in auto mode when launchd/current env enables Jarvis auto-heal, so Mac mini Jarvis readiness is restored before public monitor smoke is treated as healthy.")
    (timeout-policy jarvis-sync-worker-wait
      :default_secs 180
      :min_secs 15
      :max_secs 300
      :rule "Jarvis mobile/SSE requests may supervise a dispatched BoardTask only within this bounded synchronous wait window. If the worker has not produced durable task-result-artifact before the window expires, MissionD MUST emit JARVIS_WORKER_TIMEOUT diagnostic plus result_pending/[DONE], keep the BoardTask observable in Board/Exec, and avoid client-side hangs, non-terminal final events, or fallback answers.")
    (agent-cli-regression-policy
      :schema "missiond.agent-cli-regression-policy.v1"
      :checker "node scripts/check-v3-agent-cli-regression.mjs"
      :providers [claude-code codex agy gemini-legacy]
      :states [idle running tool-running approval-blocked auth-unavailable billing-unavailable quota-unavailable complete post-answer-feedback-complete]
    :rule "Every provider lane must have command construction, provider-specific PTY recognition, slot visibility, durable conversation source mapping, task-result-artifact completion, capability/sandbox projection, and regression fixtures before it can be promoted beyond read-only research. Codex worker lanes must inject MissionD MCP tool approval_mode=approve command overrides for mission_compute_slot, mission_context_boot, mission_context_slice, mission_shared_memory, and mission_claim_status because --ask-for-approval never does not suppress MCP approval overlays on every host. Agy/Antigravity writes durable markdown artifacts under the provider brain store (`$HOME/.gemini/antigravity-cli/brain`, overridable by MISSIOND_AGY_ARTIFACT_ROOT); Autopilot may record matching post-claim Agy brain artifacts as task_result_candidate observations but must not close until an explicit canonical task-result-artifact exists. Post-answer feedback prompts (for example Agy CLI experience survey screens) are terminal UI affordances and must not keep a BoardTask running after a canonical artifact exists. PTY structured finals, provider durable finals, and provider-empty finals are observation/candidate inputs only; they cannot create terminal BoardTask state without task_result_put + worker_settle artifact_hash. Reused provider sessions, especially Codex worker lanes backed by rollout JSONL, must ignore durable assistant messages that do not satisfy the current output contract; a later watchdog pass may settle only after the current-task canonical artifact is written. Codex PTY send timeout after prompt delivery is non-terminal: Autopilot must keep the claim, scan recent unbound codex_cli rollout conversations in the same slot project, bind the matching task_complete final back to the BoardTask, and materialize read-only Jarvis completions only when canonical task_contracts explicitly set completion_materialization_policy=autopilot_readonly_ok. Jarvis SSE must revalidate a terminal BoardTask by reading the canonical task-result-artifact; writer timeout must emit a typed diagnostic and close SSE instead of hanging after the Board already reached done. Jarvis synchronous worker supervision must also be bounded by jarvis-sync-worker-wait so a slow or stuck worker returns a typed timeout diagnostic instead of making iOS wait past the mobile request budget.")
    (portable-worker-runtime-registry
      :schema "missiond.portable-worker-runtime-registry.v1"
      :checker "node scripts/check-v3-xjpcode-portable-runtime.mjs --json"
      :rule "Portable workers are HTTP/SSE runtimes that execute a single MissionD WorkOrder and return task-result-artifact plus agent-runtime-events; they are not resident master controllers and must not bypass grounding, accepted shard, scope, write lease, or artifact-first completion gates."
      (worker :id xjpcode-readonly-worker
        :project xjpcode
        :engine xjpcode
        :mode read_only
        :runtime-env MISSIOND_XJPCODE_WORKER_URL
        :endpoint "/worker/v1/work-orders"
        :events "/worker/v1/work-orders/{id}/events"
        :health "/worker/v1/health"
        :capabilities [list_files read_file router_model_call task_result_artifact]
        :required-inputs [task_id project_id context_capsule_lisp read_scope artifact_contract]
        :forbidden [file_write apply_patch commit secret_dump direct_cli_pty_drive]
        :completion "task-result-artifact before final"
        :dispatch-surface "mission_task_delegate detects engine_hint=xjpcode/xjpcode-readonly-worker, requires MISSIOND_XJPCODE_WORKER_URL or a loopback-only xjpcode_worker_url smoke override, POSTs the scoped WorkOrder, parses SSE frames, writes task_result_put_typed, then closes through worker_settle."
        :smoke "node scripts/smoke-xjpcode-worker-dispatch.mjs --live --json"
        :smoke-rule "The smoke is explicit live opt-in because it creates a real read-only BoardTask; it may pass a loopback-only xjpcode_worker_url for local dev smoke or use daemon-visible MISSIOND_XJPCODE_WORKER_URL configured through deploy-daemon/launchd, then delegates through mission_task_delegate and polls mission_shared_memory(action=task_result_get) until a canonical task-result-artifact exists. Production smoke MUST require a terminal-success artifact status; --allow-blocked-artifact is reserved only for plumbing diagnostics so provider/router failures cannot be reported as worker success.")
      (worker :id xjpcode-code-worker
        :project xjpcode
        :engine xjpcode
        :mode write
        :status gated
        :required-inputs [accepted_shard_id write_scope write_lease worktree artifact_contract]
        :rule "Disabled until xjpcode write mode implements apply_patch, touched-file formatting, targeted tests, and MissionD work-order commit gate."))
    (startup-slot arch_maintenance
      :engine claude-code
      :lifecycle persistent
      :slot_id "slot-arch-maint"
      :role arch-maint
      :model_profile coding-default-opus-4-7
      :timeout_secs 600
      :skip_permissions false)
    (startup-slot strategy_analyst
      :engine gemini
      :lifecycle persistent
      :slot_id "slot-gemini-strategy"
      :role strategy
      :model_profile nil
      :timeout_secs 600
      :skip_permissions false)
    (startup-slot gemini_router
      :engine gemini
      :lifecycle persistent
      :slot_id "slot-gemini-router"
      :role gemini-router
      :model_profile nil
      :timeout_secs 120
      :skip_permissions false)
    (startup-slot lisp_survey
      :engine claude-code
      :lifecycle persistent
      :slot_id "lisp-surveyor"
      :role coder
      :model_profile coding-default-opus-4-7
      :timeout_secs 900
      :skip_permissions false)
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (timeout-policy claudecode-swarm
      :default_secs 600
      :min_secs 60
      :max_secs 7200)
    (timeout-policy pty-send-blocking
      :default_secs 300
      :min_secs 1
      :max_secs 7200)
    (timeout-policy dynamic-slot-spawn
      :default_secs 60
      :min_secs 10
      :max_secs 600)
    (dispatch-policy context-pack-run-wave
      :default_max_parallel 4
      :min_parallel 1
      :max_parallel 8)
    (capacity-policy swarm-workers
      :default_claude_workers 8
      :max_claude_workers 16
      :default_gemini_workers 2
      :max_gemini_workers 6
      :dynamic_slot_limit 20
      :delegate_rate_per_minute 24)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600)
    (managed-node-runtime-policy host-portability
      :mcp-config-resolution host-relative
      :mcp-config-placeholders ["$MISSION_HOME"]
      :registered-project-roots-allowed true
      :db-integer-portability [ttl_seconds extend_count message_count])
    (pty-provider-unavailable-policy provider-blocked-diagnostics
      (state auth_missing
        :state blocked
        :blocked-kind auth_missing
        :keywords [credentials credential login auth authenticated])
      (state billing_or_account
        :state blocked
        :blocked-kind billing_or_account
        :keywords [billing payment subscription suspended paused account])
      (state usage_limit
        :state blocked
        :blocked-kind usage_limit
        :keywords [quota limit rate-limit usage]))
    (policy-clause workstation-startup-slot-projection
      :owner workstation-config
      :applies-to [daemon-startup slot-manager workstation-runtime]
      :must [project-startup-slot-entries project-engine-lifecycle-slot-role-timeout-skip-permissions omit-local-startup-slot-table]
      :evidence [compiled-runtime-config daemon-slot-manager-check workstation-config-isomorphism-check]
      :checker "node scripts/check-v3-workstation-config-isomorphism.mjs"
      :legacy-rule "Daemon startup SlotManager task configs MUST be generated from workstation-config startup-slot entries.")
    (policy-clause workstation-model-profile-resolution
      :owner workstation-config
      :applies-to [compute-slot mission-task-delegate slot-spawn]
      :must [project-model-profile-spawn-model-arg caller-model-wins single-shell-token no-default-profile-model-override]
      :evidence [compiled-runtime-config compute-slot-tests workstation-config-isomorphism-check]
      :checker "node scripts/check-v3-workstation-config-isomorphism.mjs"
      :legacy-rule "mission_compute_slot model_profile resolution MUST use workstation-config model-profile spawn-model-arg, not a Rust-local profile table.")
    (policy-clause workstation-host-relative-mcp-config
      :owner workstation-config
      :applies-to [claudecode-pty blue-green-deploy dynamic-slot]
      :must [resolve-mission-home-mcp-config create-host-local-mcp-config reject-stale-jinchen-mcp-path stale-slot-clean-before-respawn]
      :evidence [blue-green-deploy-check pty-recognition-check workstation-dispatch-check]
      :checker "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"
      :legacy-rule "ClaudeCode mcp_config MUST be host-relative and PTY launch MUST resolve stale or missing xjp-mcp-config.json paths to the current host's MissionD home before spawning.")
    (policy-clause workstation-provider-unavailable-blocked
      :owner workstation-config
      :applies-to [pty-recognition board-terminal slot-state]
      :must [classify-auth-missing classify-billing-or-account classify-usage-limit preserve-blocked-provider-snapshot forbid-completed-provider-unavailable]
      :evidence [provider-unavailable-policy pty-recognition-check board-terminal-diagnostics]
      :checker "node scripts/check-v3-pty-recognition-isomorphism.mjs"
      :legacy-rule "PTY recognition MUST classify provider unavailable states as blocked diagnostics, not completed turns.")
    (policy-clause workstation-task-delegate-structured-metadata
      :owner workstation-config
      :applies-to [mission-task-delegate task-contracts board-task-description autopilot-worker-prompt]
      :must [persist-task-class-pool-engine-context-pack read-write-scope must-not-touch acceptance output-contract forbid-side-channel-only-context]
      :evidence [task-delegate-tests workstation-dispatch-check context-pack-check]
      :checker "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"
      :legacy-rule "mission_task_delegate MUST accept structured two-stage delegation metadata, persist canonical control fields into task_contracts, and render BoardTask.description only as a worker prompt projection.")
    (policy-clause workstation-swarm-project-root-resolution
      :owner workstation-config
      :applies-to [mission-swarm-run project-registry autopilot-dynamic-slot]
      :must [resolve-project-id-to-root render-target-projects merge-target-read-scope project-bound-child-boardtask absolute-context-pack-path]
      :evidence [project-registry-check workstation-dispatch-check context-pack-check]
      :checker "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"
      :legacy-rule "mission_swarm_run MUST resolve project_id to a registered project_root before creating external-project BoardTasks.")
    (policy-clause workstation-autoprovision-dynamic-slots
      :owner workstation-config
      :applies-to [mission-swarm-run mission-task-delegate dynamic-slot-autoprovision]
      :must [auto-provision-claude-dynamic-slots persist-child-assignee publish-after-slot-created explicit-auto-provision-false-only-diagnostic]
      :evidence [workstation-dispatch-check compute-slot-check autopilot-runtime-check]
      :checker "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"
      :legacy-rule "mission_swarm_run MUST auto-provision per-Claude dynamic slots by default for non-dry-run Claude context-pack / implement shards.")
    (policy-clause workstation-implementation-exact-shard-required
      :owner workstation-config
      :applies-to [mission-task-delegate mission-swarm-run implementation-worker]
      :must [require-write-scope require-context-pack-path require-accepted-shard-id forbid-broad-implementation-objective fail-fast]
      :evidence [workstation-dispatch-check context-pack-check direct-code-drift-check]
      :checker "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"
      :legacy-rule "Implementation lanes must name the accepted shard id and exact context pack before code writes.")
    (policy-clause workstation-project-root-spawn-cwd
      :owner workstation-config
      :applies-to [autopilot ensure-pty spawn-tracked-slot project-registry]
      :must [override-slot-cwd-to-boardtask-project-root require-reuse-project-root-match reject-cross-project-slot-reuse inject-project-hooks]
      :evidence [autopilot-runtime-check workstation-dispatch-check project-registry-check]
      :checker "node scripts/check-v3-autopilot-runtime-isomorphism.mjs"
      :legacy-rule "Autopilot ensure_pty MUST override pty_slot.cwd to the BoardTask.project registered project_root before PTY start.")
    (policy-clause workstation-single-running-task-per-slot
      :owner workstation-config
      :applies-to [autopilot board-task-claims slot-display]
      :must [scan-running-slot-claims before-claim unclaim-conflicting-running-task ignore-queued-unclaimed-task project-slot-active-task-from-single-claim]
      :evidence [autopilot-runtime-check workstation-dispatch-check board-isomorphism-check]
      :checker "node scripts/check-v3-autopilot-runtime-isomorphism.mjs"
      :legacy-rule "Autopilot dispatch MUST enforce a single-running-BoardTask-per-slot invariant.")
    (policy-clause workstation-durable-completion-evidence
      :owner workstation-config
      :applies-to [autopilot-close board-summary mission-execution-completion]
      :must [wait-final-settle prefer-durable-provider-final poll-bounded-durable-final sanitize-pty-fallback require-output-contract-sections]
      :evidence [autopilot-runtime-check mission-execution-check board-task-notes]
      :checker "node scripts/check-v3-autopilot-runtime-isomorphism.mjs"
      :legacy-rule "Autopilot close path MUST prefer durable provider final evidence after settle and block PTY-only closes without structured artifacts.")
    (policy-clause workstation-timeout-budget-projection
      :owner workstation-config
      :applies-to [autopilot-pty-send cc-swarm pty-send compute-slot-spawn watchdog]
      :must [project-boardtask-timeout project-claudecode-swarm-timeout project-pty-send-timeout project-dynamic-slot-spawn-timeout project-watchdog-threshold]
      :evidence [compiled-runtime-config workstation-config-isomorphism-check autopilot-runtime-check]
      :checker "node scripts/check-v3-workstation-config-isomorphism.mjs"
      :legacy-rule "Runtime wait budgets MUST project from workstation-config timeout policies rather than local fixed literals.")
    (policy-clause workstation-claim-lease-timeout-projection
      :owner workstation-config
      :applies-to [autopilot-claim-lease smart-watchdog boardtask-dispatch]
      :must [claim-lease-equals-pty-send-budget-plus-grace forbid-fixed-twenty-minute-lease preserve-legitimate-long-running-task]
      :evidence [compiled-runtime-config autopilot-runtime-check workstation-config-isomorphism-check]
      :checker "node scripts/check-v3-autopilot-runtime-isomorphism.mjs"
      :legacy-rule "Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold.")
    (policy-clause workstation-autopilot-completion-settle
      :owner workstation-config
      :applies-to [autopilot-close watchdog provider-conversation-store]
      :must [wait-worker-final-settle rebind-conversation-task-id record-provider-pty-candidates never-close-from-pty-idle-alone]
      :evidence [autopilot-runtime-check conversation-ingestion-check mission-execution-check]
      :checker "node scripts/check-v3-autopilot-runtime-isomorphism.mjs"
      :legacy-rule "Autopilot close path MUST call wait_for_worker_final_settle_window() before summary-note / mission_execution / BoardTask done writes.")
    (policy-clause workstation-research-routes-to-gemini-pool
      :owner workstation-config
      :applies-to [mission-task-delegate autopilot-routing workstation-pool]
      :must [prefer-gemini-researcher-slot for-research-intent without-explicit-claude-model forbid-claude-autoprovision-while-gemini-researcher-exists honor-explicit-claude-profile]
      :evidence [workstation-pool-check workstation-dispatch-check autopilot-runtime-check]
      :checker "node scripts/check-v3-workstation-pool-isomorphism.mjs"
      :legacy-rule "mission_task_delegate intent=research without an explicit Claude model/profile MUST prefer the workstation-pool gemini researcher slot.")
    :invariants
      ["code and research dynamic slots MUST NOT hardcode --model sonnet"
       "daemon startup SlotManager ClaudeCode task configs MUST project coder/researcher model profiles from workstation-config and omit --model for coding-default-opus-4-7"
       "daemon startup SlotManager task configs MUST be generated from workstation-config startup-slot entries, including engine/lifecycle/slot_id/role/timeout_secs/skip_permissions"
       "daemon startup MUST resolve the MissionD orchestrator root from MISSIOND_PROJECT_ROOT, MISSIOND_ORCHESTRATOR_ROOT, or the current working directory ancestor containing .missiond/v3/missiond-blueprint.lisp; runtime code MUST NOT hardcode /Users/jinchen/Projects/missiond as the orchestrator root because managed machines may install MissionD under their own user home."
       "ProjectRegistry path resolution MUST be path-component aware rather than raw string prefix matching, so sibling roots such as /Users/rickyhq/Projects/missiond-clean cannot be normalized to /Users/rickyhq/Projects/missiond. Daemon startup MUST overlay the active missiond project root from MISSIOND_PROJECT_ROOT/current blueprint root into the runtime registry before spawning provider PTYs."
       "Clean-machine daemon startup MUST create missing provider history watch directories such as ~/.claude/projects before registering filesystem watchers; absence of prior ClaudeCode/Codex/Gemini history is not a fatal condition for a managed MissionD node."
       "mission_compute_slot dynamic template role/description/mcp_config/default_cwd and allowed cwd prefixes MUST project from workstation-config slot-template + cwd-policy dynamic-slot, not a Rust-local template table; dynamic slots MUST also allow cwd under any registered active ProjectRegistry root so managed nodes installed under a different user home (for example /Users/rickyhq/Projects/missiond) can run without host-specific Lisp rewrites."
       "ClaudeCode mcp_config MUST be host-relative: workstation-config may use $MISSION_HOME/xjp-mcp-config.json, and PTY launch MUST resolve stale or missing xjp-mcp-config.json paths to the current host's MissionD home before spawning. MissionD blue-green deploy MUST create a host-local xjp-mcp-config.json with at least the missiond MCP server when it is missing, so fresh managed nodes do not fail with a literal $MISSION_HOME path. A managed node MUST NOT inherit /Users/jinchen/.xjp-mission/xjp-mcp-config.json as an executable truth. If a slot keeps a live PTY process while its public agent_info state is Exited/Error, readiness MUST classify stale_slot and spawn MUST clean the stale session before respawn instead of reporting both already-running and exited."
       "PTY recognition MUST classify provider unavailable states as blocked diagnostics, not completed turns: auth_missing covers missing credentials/login-required screens, billing_or_account covers paused/suspended/payment/subscription failures, and usage_limit covers quota/rate-limit exhaustion. Exited/Error SessionState may override stale running text, but MUST preserve these provider-unavailable blocked snapshots so Board/Terminal can show the real action required."
       "Dynamic slot database rows MUST decode ttl_seconds and extend_count portably from Postgres INTEGER or BIGINT so clean managed nodes initialized from current migrations do not panic with int4/i64 mismatches."
       "Jarvis/OpenAI-compatible chat completions default slot MUST project from workstation-config chat-completions-policy jarvis-api; X-Slot-Id remains the explicit request override and Rust MUST NOT hardcode slot-jarvis."
       "model=\"default\" and model_profile=coding-default-opus-4-7 both mean no CLI --model override"
	       "mission_compute_slot model_profile resolution MUST use workstation-config model-profile spawn-model-arg, not a Rust-local profile table"
	       "caller-supplied model wins over model_profile, but must be a single shell token"
	       "task_delegate must pass model/model_profile through to compute_slot and must not reuse an idle slot with a conflicting model override"
	       "mission_task_delegate MUST accept structured two-stage delegation metadata (task_class, pool_hint, engine_hint, context_pack_path, read_scope, write_scope, must_not_touch, acceptance), persist canonical control fields into task_contracts, and render BoardTask.description only as a worker prompt projection. Autopilot and flow runtime MUST read task_contracts/runtime ABI rather than parsing description. The generated scope_semantics line MUST state that must_not_touch forbids write/stage/commit and is not a read ban by itself; review/context-pack/research tasks MUST carry an output_contract requiring a structured artifact with Findings / Evidence / Recommendations / Verification rather than raw KB JSON or full logs."
	       "mission_task_delegate duplicate-code-worker guard MUST compare relative write_scope entries inside their resolved BoardTask.project/project_root only; cross-project sibling tasks may all write .missiond/check.sh or .missiond/evidence/current-code-mapping.md without false DUPLICATE_CODE_WORKER_BLOCKED refusals. Absolute write_scope entries remain globally comparable."
	       "mission_task_delegate MUST NOT auto-preload KB/Skill context from context_hints into worker prompts by default; current KB/skill stores are noisy and hidden prompt injection obscures task contracts. Context must come from explicit read_scope, context_pack_path, task contract, or a future explicit memory-audit workflow."
		       "Autopilot context prefetch defaults disabled until memory stores are cleaned: delegated worker prompts MUST NOT prepend KB/Skill/context-pipeline output unless an explicit memory-audit workflow opts in via MISSIOND_AUTOPILOT_CONTEXT_PREFETCH=1."
	       "mission-mcp initialize MUST NOT inject KB summary / search-path instructions by default; noisy memory context is opt-in only via MISSIOND_MCP_PRELOAD_INSTRUCTIONS=1 for explicit memory-audit sessions."
	       "Deploy/CI wait loops MUST use deploy-center provenance/events or bounded XJP MCP wait/watch tools such as xjp_build_wait and xjp_deploy_watch. Workers MUST NOT repeatedly poll GitHub Actions with raw gh api loops; GitHub API calls are diagnostic snapshots only, not the waiting mechanism."
	       "mission_swarm_run MUST honor max_gemini_workers exactly: when max_gemini_workers=0, no spawned context-pack BoardTask may use intent=research or any other routing signal that sends the task to Gemini; Claude context-pack workers use code/coder routing plus read-only completion protocol."
	       "mission_swarm_run MUST resolve project_id to a registered project_root before creating external-project BoardTasks; generated swarm metadata and default read_scope must include that project_root so Autopilot can spawn provider PTYs in the target project instead of MissionD's own cwd. For cross-project universe work, mission_swarm_run MUST accept target_project_ids/targetProjectIds, resolve every id through ProjectRegistry, render target_projects into the worker Swarm metadata and missiond.swarm-context-pack.v1 sidecar, and merge every target root into read_scope so workers can inspect all declared projects without relying on prompt prose. When target_project_ids is present and more than one context-pack worker is requested, mission_swarm_run MUST partition target project roots across workers by default, while preserving any non-target caller read_scope entries for every worker; duplicate all-target broad audits are allowed only through an explicit caller read_scope override. If a child task's read_scope/write_scope resolves to exactly one target project, the child BoardTask.project, Swarm metadata project_id/project_root, and auto-provisioned dynamic slot cwd/project_root MUST be that target project rather than the orchestrator project. Worker-facing context_pack_path MUST be an absolute MissionD workspace path (or an already absolute caller path), because external-project workers run with cwd at the target project root and relative .missiond paths would point at the wrong project. For non-dry-run dispatch, mission_swarm_run MUST materialize a missiond.swarm-context-pack.v1 sidecar at context_pack_path before publishing worker BoardTaskCreated events."
	       "mission_swarm_run MUST auto-provision per-Claude dynamic slots by default for non-dry-run Claude context-pack / implement shards and persist each created slot id as the child BoardTask assignee before publishing BoardTaskCreated. This is the productized fanout path for M6 SSOT waves; otherwise all children collapse onto the single persistent ClaudeCode slot. auto_provision_slots=false is allowed only as an explicit diagnostic override."
	       "mission_swarm_run context-pack/read-only lanes MUST render write_policy=read-only and the strict no-edit/no-stage/no-commit completion protocol even when the parent wave write_policy is lisp-first or code; write permission is granted only to lanes with a non-empty write_scope."
	       "mission_swarm_run MUST fail fast with SWARM_IMPLEMENT_WRITE_SCOPE_REQUIRED when write_policy is not read-only and the caller did not pass an explicit write_scope; the tool must never dry-run or dispatch an implementation shard with an empty write_scope because that turns disjoint ownership into prompt prose."
	       "mission_swarm_run MUST fail fast with SWARM_ACCEPTED_SHARD_REQUIRED when write_policy is not read-only and the caller did not pass accepted_shard_id/acceptedShardId; implementation workers consume already-accepted exact shards, never broad M6 objectives."
	       "mission_task_delegate MUST fail fast with EXACT_SHARD_CONTEXT_PACK_REQUIRED / EXACT_SHARD_ID_REQUIRED when a code/implementation worker declares write_scope but lacks context_pack_path or accepted_shard_id. Broad review/design goals belong to investigator lanes; implementation lanes must name the accepted shard id."
	       "Implementation worker prompts MUST explicitly forbid internal ClaudeCode TaskCreate/TaskUpdate subagent delegation; recursive decomposition belongs to MissionD master/workflow and is audited from provider durable logs as a workflow violation."
	       "mission_task_delegate and mission_swarm_run MUST accept parent_id/parentId aliases and persist them into CreateBoardTaskInput.parent_id when a master objective spawns child shards; mission_swarm_run MUST also render parent_board_task_id in the worker-facing Swarm metadata so the Board hierarchy, parent-note closeout, and master recovery loop can connect child completion evidence back to the active objective."
	       "Autopilot ensure_pty MUST override pty_slot.cwd to the BoardTask.project's registered project_root when the BoardTask carries a project label that resolves under ProjectRegistry and that root differs from slot.config.cwd; spawn_tracked_slot's project-root-spawn-cwd contract then handles Gemini/Codex hard-fail and ClaudeCode normalization. Slot reuse for cross-project dispatch MUST require slot.project_root == BoardTask project_root (already enforced for mission_task_delegate; mission_swarm_run BoardTasks rely on the spawn-side cwd override for the same effect)."
	       "Codex worker lanes MUST pass the resolved slot project root through the Codex CLI `--cd` workspace flag in addition to PTY process cwd; Codex profile/resume state must not silently select a stale local repository when MissionD metadata says the slot is running in a managed project root."
	       "Autopilot MUST unclaim a BoardTask when ensure_pty returns false after a claim, because spawn-pending / busy / transient PTY states are retryable dispatch conditions and must not wedge the task in running with no prompt delivered."
	       "Autopilot ensure_pty MUST treat `PTY session already running` from spawn_tracked_slot as a pre-provisioned dynamic-slot race: wait briefly for Idle and reuse the PTY instead of recording a spawn failure note or incrementing BoardTask retry. If the session remains non-idle, it is a retryable busy condition and the task must be unclaimed for a later tick."
	       "Autopilot/flow-engine BoardTask dispatch MUST bind conversations.task_id to the active BoardTask via a bounded retry helper at dispatch time (5 attempts at 200 ms) and MUST re-bind after the worker final settle window to cover provider JSONL/session-discovery races; completion-time durable_provider_completion_for_slot_task remains a fallback. The dispatch site may overwrite conversations.task_id only for the slot's currently active, not-ended provider conversation; it MUST NOT rebind a completed historical conversation left in slot_sessions from a previous prompt. The conservative `conversation_task_binding_update_allowed` predicate is reserved for the post-completion durable backfill path (set only when unbound or already matching); pre-dispatch binding logs any displaced task id for audit but skips completed sessions. Durable final selection MUST also reject provider conversations whose ended_at is earlier than the BoardTask claimed_at, even if a stale conversations.task_id pointer was already corrupted. Historical attribution lives in durable messages (mission_conversation_query(taskId=...) MUST also recover provider conversations whose durable messages contain the BoardTask id), so a stale conversations.task_id pointer is unnecessary and previously caused mission_conversation_query(taskId=<new>) to return the prior task's conversation (BoardTask 31e5449c-e315-4003-ad59-c3eebd5eb837 evidence: slot-claude-code-default returned the 5599b07a conversation when queried by 738c96f5; Jarvis smoke f0708d99... later exposed stale final d15c186c... closing a new task)."
       "Autopilot dispatch MUST enforce a single-running-BoardTask-per-slot invariant: before claim_board_task, scan running tasks for any other BoardTask whose claim_executor_type=pty_slot and claim_executor_id matches the incoming slot id, and unclaim each one with a durable note. A queued task with assignee=slot but no claim_executor_id is NOT considered running on the slot and MUST NOT be unclaimed by this guard. The display layer (handlers/compute/slot.rs `active_board_task_for_slot`) projects the slot's running task from this single-claim invariant, so two tasks can never appear running on the same slot for the same dispatch tick."
       "Autopilot PTY/provider close classifiers are diagnostic observation producers only. Delegated worker BoardTasks carry dispatch/output contracts in runtime_metadata, never Markdown description; pty_only_close_blocker and output_contract_close_blocker may tag task_result_candidate quality, but they cannot produce terminal BoardTask state. The only close path is canonical task_result_artifact lookup followed by capability-checked worker_settle with the exact artifact_hash. Repro evidence from BoardTask 31e5449c..., 7b5f3174..., and memory review e1ea8d06... remains covered as observation-quality filtering rather than completion authority."
       "Autopilot durable final acceptance evidence MUST recognize provider final summaries that say gates green/pass, checks pass, checker passed, check.sh passed, acceptance commands passed, or final M10 evidence-only gate confirmation, not just legacy words like verified/passed/changed files. Repro evidence: M10 child tasks 5ecb01cd..., d699c9c7..., 66aed32d..., ae72b0ec..., and f6c5475d... produced durable ClaudeCode finals with gate/checker completion language but were incorrectly blocked as missing-acceptance-evidence."
       "mission_swarm_run callers (resident master, autopilot, ad-hoc operators) MUST pass multi-project objectives via target_project_ids/targetProjectIds/target_projects/targetProjects structurally; project lists embedded only in the objective prose are ignored by the tool because the schema does not parse natural language. The MCP schema MUST expose target_project_ids and aliases as an array property of mission_swarm_run so MCP clients can pass it without guessing naming conventions. Failure mode (BoardTask 31e5449c regression): when only project_id was supplied and the objective text named multiple registered projects in prose, target_projects collapsed to project_id only and the swarm could not fan out across the universe."
	       "Autopilot MUST treat explicit engine_hint/pool_hint as hard constraints when the V3 workstation-pool declares at least one matching worker: resolve matching workers against the full workstation-pool before task_class fallback can narrow candidates away; dispatch hint matching MUST normalize underscore and hyphen spelling so claude_code and claude-code are equivalent and do not create false reroute notes; if that worker is busy or stopped, the task waits instead of silently spending a different provider. Fallback to a non-matching worker is allowed only when no declared worker satisfies the hint at all, and that fallback MUST record a durable reroute_reason as a BoardTask note before dispatching so the operator can see why the requested engine/pool was not used. Autopilot close-owner MUST block task close when a worker final says it could not write the requested deliverable because of plan/read-only mode."
	       "mission_task_delegate intent=research without an explicit Claude coding model/model_profile MUST prefer the workstation-pool gemini researcher slot (slot-gemini-ultra) when registered; the researcher slot-template's :default-model-profile is research-default, which binds to the gemini-ultra worker. Auto-provisioning a dynamic Claude slot for research is forbidden while a V3 gemini researcher slot exists; the BoardTask is queued unassigned and the autopilot routes it to the gemini slot once idle. Explicit model_profile=coding-default-opus-4-7 (or any Claude profile) still routes the BoardTask to Claude."
       "Project-bound workstation spawn MUST materialize MissionD Claude hook executables in ~/.claude/hooks before syncing <project>/.claude/settings.local.json; settings MUST never point at missing missiond-session-register.sh or missiond-context-inject-v2.sh. The spawn path MUST inject MISSION_IPC_ENDPOINT into the slot env; this preserves global ~/.claude/settings.json while making SessionStart UUID capture local, idempotent, and project-scoped. UserPromptSubmit context prefetch MUST be opt-in only through MISSIOND_CLAUDE_CONTEXT_PREFETCH=1 and the hook sync MUST remove missiond-context-inject-v2.sh from project settings by default while KB/history stores are noisy."
       "Autopilot pty.send budget MUST project from BoardTask.timeout_secs (default 1800s, clamped 60..7200) — never a fixed 600_000ms — so a delegated long-running task gets the timeout the delegator already declared"
       "mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm (default 600s, clamped 60..7200) — never a local 600_000ms literal"
       "mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking (default 300s, clamped 1..7200) — never a local 300_000ms literal"
       "mission_compute_slot and Claude/Gemini slot-orchestrator dynamic slot spawn wait_for_idle timeouts MUST project from workstation-config timeout-policy dynamic-slot-spawn (default 60s, clamped 10..600) — never local Some(60)/Some(120) literals"
       "context-pack-run-wave default worker fanout MUST project from workstation-config dispatch-policy context-pack-run-wave (default 4, clamped 1..8), while caller --max-parallel remains an explicit override"
       "mission_swarm_run fanout defaults/caps and dynamic slot limits MUST project from workstation-config capacity-policy swarm-workers (default Claude 8 max 16, default Gemini 2 max 6, dynamic slots 20, delegate rate 24/min) so supervised waves can scale without divergent Rust constants"
       "Dynamic slot TTL and per-request extension budget MUST project from workstation-config ttl-policy dynamic-slot (create default 14400s, clamped 300..28800; extend default/max 3600s) — direct mission_compute_slot create/extend and delegated task_delegate auto-provision must not hardcode the TTL window"
       "Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget plus a small grace (default 120s); only the no-PTY-session branch may reclaim sooner so a missing process can never wedge the slot"
       "Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold (projected pty.send budget plus grace); the legacy fixed 20-minute lease is forbidden because it lets the watchdog reclaim a slot whose claim is still legitimately ticking inside the declared timeout"
	       "Autopilot summary-note source is a projection of canonical task_result_artifact data. wait_for_worker_final_settle_window(), per-session reconcile, await_durable_provider_completion_for_slot_task, provider_completion_summary_for_task, and fallback extract_worker_final_summary(res.response, full_prompt) may only record observation/candidate evidence; they must not decide terminal state. Raw res.response remains forbidden in the **Autopilot 执行完成** note format string and mission_execution completion summaries; fallback extraction must strip bare Bash(...)-style tool-call lines, tool logs, echoed task contracts, and `[Pasted text +N lines, paste again to expand]` collapse markers before writing candidate evidence. Auth-error and quota-exhausted diagnostic notes intentionally bypass this projection path and keep raw platform errors for on-call visibility."
	       "Autopilot final-summary filtering MUST reject common progress narration and retry-noise markers such as `I'll begin by reading`, `Let me ...`, `wakeup will fire`, and `no space left on device`; EventBus task completion payloads such as event_msg.task_complete.last_agent_message are observation inputs only and still require canonical task-result-artifact settlement."
       "Autopilot close path MUST call wait_for_worker_final_settle_window() after pty.send completion only as an observation collection budget. PTY idle, pty.send completion, durable provider evidence, and sanitized fallback summaries are task_result_candidate inputs; BoardTask done still requires canonical task_result_artifact hash plus worker_settle."
       "If pty.send returns an active/progress frame but the provider later records a durable final assistant message in claude-jsonl/codex-local-index/gemini-chat-file, or the master/worker later writes a BoardTask summary note, and the claimed slot is idle/exited/error, Autopilot watchdog may record a task_result_candidate observation and may settle only after an existing canonical completed task-result-artifact hash passes worker_settle; it MUST NOT wait for PTY idle, provider prose, or Board notes to become terminal authority. A terminal provider slot (Exited/Error) is not a busy state; after canonical artifact lookup fails, it may be unclaimed by the same watchdog recovery policy instead of wedging the task indefinitely."]
    (prompt-tool-contract autopilot-claudecode-prompt
      :applies-to [coder researcher ops]
      :always-shown
        ["Board Task ID is surfaced in every dispatched prompt so the worker (and any reader of the prompt snapshot) can audit which BoardTask the slot is executing."]
      :objective-dedupe
        "Prompt assembly MUST suppress duplicated objective text: when BoardTask.description equals BoardTask.title or starts with the title followed only by blank lines, the assembled prompt uses description alone; distinct title + description still renders both joined by a blank line."
      :artifact-owned-close
        (:mode orchestrator-owned
         :worker-contract "Workers receive the BoardTask id for audit and return structured final output. They MUST NOT be instructed to call mission_board_update(status=\"done\") as the primary close path."
         :projection-note "If a worker has board MCP tools, mission_board_note_add may be used only as an optional projection/evidence note. Board notes are not the canonical result."
         :close-owner "Autopilot/orchestrator closes only after canonical task-result-artifact validation and worker_settle artifact_hash."
         :rationale "Jarvis and external interaction channels need a stable task-result-artifact before returning client-visible results; worker self-close can bypass artifact settlement and produce stale Board/phone state.")
      :non-prompt-guidance
        ["Decision Engine escalation suffix (mission_question_create) and ops-task focus suffix remain behaviorally intact and are appended after the deduplicated base prompt."])
    (execution-ownership delegated-boardtask
      :applies-to [coder researcher ops]
      :prompt-owner
        "For delegated BoardTask execution, Autopilot is the sole task-prompt owner. mission_task_delegate auto-provision (compute_slot/spawner) MAY warm a dynamic slot but MUST NOT send the task objective as a fire-and-forget initial-prompt; objective is slot metadata only. The slot starts idle and Autopilot sends the BoardTask prompt via state.pty.send. Direct compute_slot warmup requires an explicit initial_prompt field."
	 :close-owner
        (:default "Autopilot is the prompt/observation owner; it may record PTY/provider output as task_result_candidate observations, but BoardTask running→done requires worker_settle/equivalent typed settle with a canonical artifact_hash."
         :legacy-worker-self-close "Hard cutover disables legacy worker self-close as control authority. A BoardTask already marked Done without canonical artifact remains a projection inconsistency and Jarvis/external clients still require task-result-artifact validation."
         :execution-log-synthesis "If the delegated prompt carries a pre-opened mission_execution log and the slot returned a final summary without appending a completion, Autopilot may record an observation/candidate, but it MUST NOT synthesize task completion or close the BoardTask without canonical task-result-artifact."
	         :summary-note-source "The `**Autopilot 执行完成**` BoardTask summary note is a projection of canonical task-result-artifact data. Durable provider final evidence, PTY screens, and extract_worker_final_summary are allowed only as diagnostic/candidate producers and must not decide terminal state."
	         :settle-window "Settle windows and durable provider polling are observation collection budgets only; they cannot create BoardTask done state. Done requires canonical artifact hash plus capability-checked worker_settle."
		         :commit-failure-blocker "If the worker final indicates a blocking commit/tool failure such as GPG pinentry cancellation, commit failed, failed to commit, or could not commit, Autopilot MUST NOT mark the BoardTask done or synthesize a successful mission_execution completion. It writes an autopilot observation/blocker note only; terminal BoardTask state still requires typed events and worker_settle with canonical evidence."
		         :idle-durable-summary-close "If pty.send returned an active/progress frame and left the BoardTask running, a later watchdog tick may record observations/candidates from provider logs or summary notes, but may close only after an existing canonical completed task-result-artifact hash passes worker_settle."
	         :blocked "If the task transitioned to Blocked (e.g. mission_question_create) during execution, Autopilot preserves the Blocked state on pty.send return and never overwrites it with done.")
      :dispatch-guard
        "The per-slot dispatch guard MUST be held across the entire state.pty.send call; the legacy release-before-send pattern allowed a second caller to dispatch to the same slot mid-flight. The guard is per-slot, so holding it does not starve callers targeting other slots."
      :concurrent-slot-dispatch
        "Autopilot dispatch_board_tasks MUST start state.pty.send work concurrently across different slots and MUST NOT wait for worker turn completion inside the dispatch tick. The legacy serial loop awaited one slot's pty.send before any other slot's send could begin, and the later JoinSet-drain variant still starved newly-idle pre-provisioned dynamic slots because the tick did not return until early workers finished. The implementation MUST hand each ready BoardTask's send + post-send tail to a detached tokio task with an OwnedSlotDispatchGuard moved in, so same-slot exclusion covers the entire send + close-owner / KB-feedback / deploy-review sequence while the Autopilot event loop can keep dispatching other idle slots on later ticks. Quota / global-pause / KB-feedback / retry semantics run inside that background tail and MUST update durable BoardTask/event state instead of relying on the dispatch tick return."
      :restart-recovery
        "Restart recovery MUST clear stale slot-dyn-* BoardTask assignee pins when the runtime slot is absent and the dynamic_slots row is not active, using BoardStore::clear_board_task_assignee before normal no-assignee routing resumes."
	    :rationale
	        "Wave33 evidence: a delegated BoardTask was sent twice — once via spawner.initial_prompt fire-and-forget, then again via Autopilot pty.send — and the slot's TextOutputEvent::Complete arrived without Autopilot transitioning the BoardTask to done. Single ownership of prompt+close eliminates the orphaned-task class entirely.")
    (claude-code-mcp-recovery
      :desc "Lisp-owned ClaudeCode MCP reconnect ritual and missing-MCP incident contract; the mounting and reconnect navigation are Lisp-pinned, never tool-registry-decided at runtime."
      :reconnect-keystrokes ["/mcp" "<enter>" "<arrow-down>*N" "<enter>" "<enter>"]
      :forbid-numeric-shortcut true
      :missing-incident-kind "claude_code_mcp_missing"
      :reconnect-failed-incident-kind "claude_code_mcp_reconnect_failed"
      :reconnect-budget-attempts 1
      :wake-resident-master true
      :surfaces ["crates/missiond-pty/src/session.rs::Session::mcp_reconnect_sequence"
                 "crates/missiond-pty/src/session.rs::PTYSession::mcp_reconnect"
                 "crates/missiond-pty/src/manager.rs::PTYManager::mcp_reconnect"
                 "crates/missiond-daemon/src/workers/local/pty_event_worker.rs::handle_mcp_tool_error"
                 "crates/missiond-daemon/src/engine/master_control.rs::spawn_incident_event_sub"]
      :rationale "Claude Code's /mcp picker numeric shortcuts have shifted between TUI versions; arrow-key navigation is the only stable substrate. When supports_mcp=true is advertised but no mission_* tool surfaces after slot ready, the worker is operating without orchestration tools and the master must be woken via a durable incident, not a silent reconnect retry loop."))

  (workstation-policy-shards
    :desc "Split workstation-config invariants into policy shards so runtime, checkers, and agents can reason over one small policy at a time."
    (policy slot-lifecycle-policy
      :owns [startup-slot dynamic-slot ttl heartbeat release stale-slot-reap]
      :core ((step s1 :logic "reserve or create a slot with projected model/cwd/ttl")
             (step s2 :logic "bind active task, conversation, and claim lease")
             (step s3 :logic "heartbeat while provider is running")
             (step s4 :logic "release or mark stale after durable completion or TTL expiry"))
      :surface workstation-config)
    (policy delegation-contract-policy
      :owns [task_delegate swarm_run accepted_shard write_scope read_scope completion_protocol]
      :core ((step s1 :logic "classify investigator versus implementer before dispatch")
             (step s2 :logic "require context_pack_path and accepted_shard_id for implementation lanes")
             (step s3 :logic "persist read/write scope and must-not-touch into BoardTask metadata")
             (step s4 :logic "reject broad objectives in code-worker lanes before provider spawn"))
      :surface workstation-config)
    (policy completion-authority-policy
      :owns [provider_final provider_artifact task_result_artifact board_note_projection pty_diagnostic]
      :core ((step s1 :logic "record durable provider final and PTY output as observation/candidate evidence only")
             (step s2 :logic "for Agy, read post-claim Antigravity brain markdown artifact as candidate evidence; an explicit BoardTask ID inside the artifact is first-authority attribution and foreign BoardTask artifacts must be rejected before broad keyword matching")
             (step s3 :logic "accept only explicit task_result_put canonical artifacts")
             (step s4 :logic "project concise summary to Board note and mission_execution")
             (step s5 :logic "close BoardTask only through worker_settle artifact_hash and capability/write-scope validation"))
      :surface task-result-artifact)
    (policy cross-project-dispatch-policy
      :owns [project_root cwd read_scope target_project_ids external_project_worker]
      :core ((step s1 :logic "resolve every project id through ProjectRegistry")
             (step s2 :logic "materialize absolute context_pack_path and project roots")
             (step s3 :logic "spawn provider with target project cwd or explicit readable scope")
             (step s4 :logic "record reroute or block reason when project evidence is missing"))
      :surface project-registry)
    (policy context-prefetch-policy
      :owns [kb_prefetch skill_prefetch explicit_context memory_audit]
      :core ((step s1 :logic "default to explicit context_pack/read_scope only")
             (step s2 :logic "allow KB/skill prefetch only in explicit memory-audit or operator-approved sessions")
             (step s3 :logic "redact noisy or unreviewed memory before worker prompt projection")
             (step s4 :logic "record prefetch source and reason as diagnostic evidence"))
      :surface memory-kb)
    (policy mcp-recovery-policy
      :owns [mcp_ready reconnect_ui navigation_hint provider_tool_availability]
      :core ((step s1 :logic "detect missing MCP readiness from slot/provider diagnostics")
             (step s2 :logic "surface human-like reconnect navigation hints without numeric shortcut assumptions")
             (step s3 :logic "route repeated failure to BoardTask and not hidden prompt retry")
             (step s4 :logic "only resume worker dispatch once readiness is verified"))
      :surface capability-governance)
    :checker "node scripts/check-v3-workstation-dispatch-isomorphism.mjs")

  (workstation-pool
    :desc "Compact V3 SSOT for human-owned external compute accounts exposed as MissionD workers."
    :account-mode single-login
    :selection [task_class capability idle_state same_slot_guard]
    :evidence ".missiond/v3/evidence/workstation-pool.lisp"
    (worker claude-code-default
      :engine claude-code
      :role coder
      :slot-id "slot-claude-code-default"
      :task-type claude_code_default
      :model-profile coding-default-opus-4-7
      :model nil
      :task-classes [code implementation review context-pack ops]
      :capabilities [code-read code-write scoped-commit mcp]
      :max-concurrency 1
      :timeout-secs 1800
      :default-use code-implementation
      :accepts-boardtask true
      :write-allowed true)
    (worker claude-code-deploy-ops
      :engine claude-code
      :role deploy-ops
      :slot-id "slot-claude-code-deploy-ops"
      :task-type claude_code_deploy_ops
      :model-profile coding-default-opus-4-7
      :model nil
      :task-classes [deploy-ops deployment ops incident-response]
      :capabilities [deploy-read deploy-observe deploy-center-query rollback-plan mcp]
      :max-concurrency 1
      :timeout-secs 2400
      :default-use deployment-operations
      :accepts-boardtask true
      :write-allowed false)
    (worker claude-code-fast-patch
      :engine claude-code
      :role patcher
      :slot-id "slot-claude-code-fast-patch"
      :task-type claude_code_fast_patch
      :model-profile daily-sonnet
      :model nil
      :task-classes [patch test chore low-risk-fast-path]
      :capabilities [code-read code-write scoped-commit narrow-patch mcp]
      :max-concurrency 1
      :timeout-secs 900
      :default-use narrow-patch
      :accepts-boardtask true
      :write-allowed true)
    (worker gemini-ultra-pro
      :engine gemini
      :role researcher
      :slot-id "slot-gemini-ultra"
      :task-type gemini_ultra
      :model-profile gemini-ultra-pro-preview
      :model nil
      :approval-policy plan
      :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
      :task-classes [research review context-pack lisp-compression general]
      :capabilities [read-only analysis design-review]
      :max-concurrency 1
      :timeout-secs 900
      :default-use research-review
      :accepts-boardtask true
      :write-allowed false)
    (worker gemini-fast-survey
      :engine gemini
      :role survey
      :slot-id "slot-gemini-fast-survey"
      :task-type gemini_fast_survey
      :model-profile nil
      :model "gemini-2.5-flash"
      :approval-policy plan
      :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
      :task-classes [survey summary mechanical-scan]
      :capabilities [read-only summary]
      :max-concurrency 1
      :timeout-secs 600
      :default-use low-authority-survey
      :accepts-boardtask true
      :write-allowed false)
    (worker agy-research
      :engine agy
      :role researcher
      :slot-id "slot-agy-research"
      :task-type agy_research
      :model-profile nil
      :model nil
      :task-classes [research review context-pack survey]
      :capabilities [read-only analysis design-review provider-successor]
      :max-concurrency 1
      :timeout-secs 900
      :default-use agy-read-only-research
      :accepts-boardtask true
      :write-allowed false)
    (worker codex-code-worker
      :engine codex
      :role coder
      :slot-id "slot-codex-code-worker"
      :task-type codex_code_worker
      :model-profile codex-master-gpt-5-5-xhigh
      :model nil
      :reasoning-effort xhigh
      :search true
      :sandbox workspace-write
      :approval-policy never
      :task-classes [code implementation design review regression-analysis]
      :capabilities [code-read code-write scoped-commit shell-exec search]
      :max-concurrency 1
      :timeout-secs 1800
      :default-use codex-code-shard
      :accepts-boardtask true
      :write-allowed true)
    (worker codex-review-worker
      :engine codex
      :role reviewer
      :slot-id "slot-codex-review-worker"
      :task-type codex_review_worker
      :model-profile codex-master-gpt-5-5-xhigh
      :model nil
      :reasoning-effort xhigh
      :search true
      :sandbox read-only
      :approval-policy never
      :task-classes [review architecture-review regression-analysis]
      :capabilities [code-read design-review search]
      :max-concurrency 1
      :timeout-secs 1200
      :default-use codex-review-shard
      :accepts-boardtask true
      :write-allowed false)
    (worker codex-master-control
      :engine codex
      :role orchestrator
      :slot-id "slot-codex-master-control"
      :task-type codex_master_control
      :model-profile codex-master-gpt-5-5-xhigh
      :model nil
      :reasoning-effort xhigh
      :search true
      :sandbox danger-full-access
      :approval-policy never
      :task-classes [master-control orchestration governance night-audit]
      :capabilities [board-write kb-write execution-log dispatch code-read code-write shell-exec search mcp full-access]
      :max-concurrency 1
      :timeout-secs 7200
      :default-use resident-master-control
      :accepts-boardtask false
      :write-allowed true)
    :invariants
      ["Claude coding workers use coding-default-opus-4-7, which means no Claude Code --model override; Sonnet cannot be the coding default."
       "Codex master-control is a resident orchestrator lane with full local sandbox access, not a normal BoardTask code shard candidate; it may directly repair MissionD control-plane issues when necessary, but every direct mutation must leave Board/KB/checkpoint evidence and ordinary implementation still prefers delegated workers."
       "Deployment/Ops work MUST route to the explicit claude-code-deploy-ops lane when context_intent=deploy-ops or task_class=deploy-ops is present; generic claude-code-default is not the default deployment lane. Deploy-ops is observation/planning first: it may query deploy-center, provenance, skills, and logs, but production mutation requires deploy-center policy or explicit Board approval. CI/build waiting uses deploy-center events/provenance or bounded XJP MCP wait/watch tools; raw gh api polling loops are forbidden."
       "Claude fast-patch may use Sonnet only for narrow atomic tasks whose context-pack already identifies exact files/regions; it is not a default coding lane."
       "Autopilot must resolve workstation-pool dispatch by task class before hints: `pool_hint` may select a concrete worker across the full pool, but `engine_hint` alone only filters/ranks the current task-class candidates and MUST NOT widen a complex `task_class=code` shard into the `claude-code-fast-patch` Sonnet lane."
       "Gemini Ultra Pro is the high-language read-only investigation lane using gemini-3.1-pro-preview; Gemini fast survey is explicitly low-authority mechanical scan/summary work."
       "Agy is the successor research CLI lane. It starts as read-only until PTY recognition, durable-log mapping, and task-result-artifact smoke prove it can safely take implementation shards."
       "Gemini is initially read-only: research, review, context-pack, and Lisp compression advice may route there; scoped write/commit work stays on Claude until a separate Gemini write smoke passes."
       "Codex code/review worker lanes are ordinary BoardTask candidates and are separate from codex-master-control; codex-code-worker uses workspace-write rather than danger-full-access, codex-review-worker uses read-only, and the resident master cannot consume ordinary shard work."
       "Read-only Gemini pool workers MUST project to Gemini CLI `--approval-mode plan --policy .missiond/v3/policies/gemini-readonly-policy.toml`; workstation-pool registration MUST NOT set dangerously_skip_permissions/YOLO for ordinary BoardTask workers, regardless of write_allowed, and the policy MUST deny subagent delegation and write/shell tools."
       "Autopilot unassigned BoardTasks select from workstation-pool by task class before considering any legacy slot; old slots.yaml Sonnet entries are not generic coding candidates."
       "mission_compute_slot action=list must expose workstation_pool with runtime slot presence and idle/busy/stopped status."
       "Supervisor patrol (slot-supervisor) is gated on V3 workstation-pool / runtime-config registration; absent a supervisor worker entry the patrol stays inert and MUST NOT call ensure_memory_slot_by_id, so the legacy 'Memory slot not configured in slots.yaml' warning cannot fire."
       "V3 workstation-pool (plus startup-slots) is authoritative for dispatchable slots; mission_compute_slot list MUST tag any static slot whose id is not in the V3 projection as legacy=true and dispatchable=false (or split it into legacy_static_slots) so retired Sonnet entries (autopilot/topology-guardian/extraction-worker/delta-validator/...) cannot resurface as candidates."
       "All generated or compatibility CliEngine enums MUST cover the canonical provider set ClaudeCode, Gemini, Codex, and Agy, with hyphen/underscore input normalization at API boundaries; no stale two-variant ClaudeCode/ClaudeMd enum may be used as dispatch authority."
       "mission_compute_slot list status MUST derive from PTYManager (state.pty.get_status) for every slot it surfaces, so it cannot contradict mission_pty_status for V3 pool slots; the SlotManager session_id field is only a fallback when no PTY status exists, and it MUST NOT report 'running' when no PTY is attached."]
    :checker "node scripts/check-v3-workstation-pool-isomorphism.mjs")

  (agent-interaction-policy
    :schema "missiond.agent-interaction-policy.v1"
    :desc "Prompt style is an interaction aid, not the safety boundary. Agents receive heuristic questions and prepared context; hard constraints live in BoardTask metadata, workflow Lisp, checker gates, and Rust runtime guards."
    (role resident-master
      :style heuristic-review
      :intake-question "关于用户提出的这个问题或需求，我还有哪些不知道的信息？这些未知信息分别应该从 SSOT、skill operational facts、项目代码、部署事实、事件总线、还是用户决策入口取得？"
      :intent-question "基于已补齐的证据，请判断用户此刻的真实意图、长期偏好或治理原则是什么；若判断成立，产出带 evidence_refs/confidence/supersession_scope 的 intent_memory_candidate；高置信意图必须写入 memory:decision，低置信只进入 needs-review/candidate。"
      :prompt-question "请审视当前目标和 SSOT Lisp：颗粒度是否足够细？哪些架构可以更优雅？你还需要哪些证据、调查工位或 exact shard？"
      :required-output-fields [decision reasoning_summary unknowns inferred_user_intent intent_memory_candidate evidence_needed delegation_plan? next_question_or_action]
      :default-inputs [active-boardtask context-pack-path current-phase event-summary]
      :forbidden-default-inputs [kb board-backlog historical-conversation provider-durable-logs]
      :rule "resident-master must complete unknowns-first-intake, intent-inference, intent-memory-capture, review-question, and evidence-plan before investigation/implementation unless the active BoardTask explicitly declares exact-shard-ready=true. Intent memory capture records the judged intent with evidence and confidence: high-confidence stable intent is written through mission_kb_remember(category=memory:decision), while uncertain intent stays as needs-review/candidate artifact. This does not re-enable broad KB prompt preloading.")
    (role investigator-worker
      :style context-prepared-question
      :prompt-opening "请审视、比较、找缺口并给出建议；只使用给定 read_scope/context_pack_path 的证据。"
      :metadata [BoardTask-ID project_id context_pack_path read_scope write_scope must_not_touch acceptance completion_protocol]
      :output-contract [Findings Evidence Recommendations Verification]
      :rule "investigator workers are read-only by default and produce context-pack evidence, not implementation patches or raw KB/log dumps.")
    (role implementer-worker
      :style accepted-shard-question
      :prompt-opening "基于已接受 shard 和上下文，请完成这个最小同构改动；保持现有行为，只在 declared write_scope 内修改。"
      :metadata [BoardTask-ID project_id context_pack_path read_scope write_scope must_not_touch acceptance completion_protocol]
      :output-contract [Changed-Files Acceptance-Evidence Residual-Risk]
      :rule "implementer workers may write only declared write_scope; exact shard ownership is enforced by task_delegate/Autopilot, not by prompt prose.")
    (role deterministic-llm-tool
      :style precise-instruction
      :rule "non-agent deterministic LLM calls may use exact prompts because they do not autonomously dispatch tools or workers.")
    :runtime-invariants
      ["Prompt text MUST NOT be the primary safety boundary; read/write scope, acceptance, pool routing, and closure policy are structured BoardTask/runtime fields."
       "Worker prompts MUST NOT prepend KB/Skill/history/provider-log context unless an explicit memory-audit workflow opts in."
       "M6 workflows MUST materialize context-pack artifacts with questions, hypotheses, evidence_needed, findings, design_options, and accepted_shards before implementation shards."
       "Direct implementation from an initial objective is allowed only when exact-shard-ready=true is explicitly present in the BoardTask metadata or description."]
    :checker "node scripts/check-v3-workflow-isomorphism.mjs")

  (codex-boot-context-policy
    :schema "missiond.codex-boot-context-policy.v1"
    :purpose "Every resident Codex, Codex worker, or external Codex handoff should start from a small validated capsule instead of inheriting a giant historical chat or hoping the agent reads a repo note."
    :capsule ".missiond/v3/evidence/codex-boot-context.lisp"
    :mcp mission_context_boot
    :layers
      ((layer L0-always-on :source ".missiond/v3/evidence/codex-boot-context.lisp" :rule "Always inject the shared collaboration protocol: Lisp SSOT, intent->plan->work-order, unknowns-first grounding, exact shard/write lease, and durable evidence completion.")
       (layer L1-current-task :source [mission_master_status BoardTask work-order] :rule "Load only active objective, BoardTask/work-order id, project_id, context_pack_path, accepted_shard_id, and checkpoint.")
       (layer L2-grounded-facts :source mission_context_gather :rule "Query SSOT/project registry/skill evidence/active memory/infra/tool directory only for explicit unknowns.")
       (layer L3-cold-evidence :source [raw-conversations historical-board provider-logs runtime-reports] :rule "Cold evidence is opt-in for audit/debug and must not be startup prompt preload."))
    :rules
      ["Boot capsule is a compact contract and MUST NOT contain secrets, raw provider logs, bulk chat history, or unreviewed KB dumps."
       "mission_context_boot is the public retrieval surface for external new conversations and resident/worker startup."
       "Every confirmed durable user intent that should survive a fresh conversation becomes an intent_memory_candidate, then active memory only after review or high-confidence evidence."]
    :checker "node scripts/check-v3-codex-boot-context-isomorphism.mjs")

  (skill-edit-delegation-policy
    :schema "missiond.skill-edit-delegation-policy.v1"
    :purpose "Keep ClaudeCode/Codex/Gemini skill files as operational knowledge managed by the workstation that understands that ecosystem best."
    :authority
      ((reader resident-master)
       (planner mission_context_gather)
       (editor claude-code-skill-maintainer)
       (reviewer codex-or-resident-master))
    :rules
      ["Codex/resident-master may read skill files as evidence through mission_skill_context or skill evidence artifacts."
       "Mutating skill files under ~/.claude/skills, ~/.codex/skills, or project skill directories MUST be represented as a BoardTask/work-order and delegated to a ClaudeCode skill-maintainer or deploy-ops lane."
       "Skill edits require context_pack_path, read_scope, write_scope, completion protocol, and a task-result-artifact; direct local edits by the resident master are not an accepted path."
       "When a user asks for skill changes, first gather related skill evidence, then create an intent.lisp/plan.lisp work-order or exact shard for ClaudeCode."]
    :checker "node scripts/check-v3-memory-kb-isomorphism.mjs")
