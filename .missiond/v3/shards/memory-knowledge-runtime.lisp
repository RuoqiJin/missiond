  (capability-governance-policy
    :desc "Lisp-owned capability audit policy; runtime review paths and protected lists are projections, not Rust-only constants."
    :review-sidecar ".missiond/v3/runtime/capability-usage-review.json"
    :protected-tool-patterns ["mission_execution"
                              "mission_intent"
                              "mission_forge_"
                              "mission_sys_"
                              "mission_daemon_update"
                              "mission_health"
                              "mission_power_control"
                              "mission_kb_ops"
                              "mission_audit"
                              "mission_pty_signal"
                              "mission_pty_key"
                              "mission_pty_text"
                              "mission_pty_confirm"
                              "mission_incident"]
    :protected-flow-patterns ["engineering"
                              "F-execution-log-governance"
                              "F-incident-reaction"
                              "F-capability-usage-monitoring"]
    :invariants
      ["mission_capability_usage snapshot/report/candidates/mark/ack MUST project review sidecar location and protected source/target policy from capability-governance-policy."
       "Protected pattern semantics stay explicit: tool patterns ending '_' are prefixes; other tool patterns are exact; flow patterns match exact or prefix."
       "A real MissionD project with .missiond but no capability-governance-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (mcp-tool-governance-policy
    :desc "Primary MCP tool families for agents; old public tools remain callable compatibility leaves, but agents should select by family first."
    :schema "missiond.mcp-tool-governance.v1"
    :primary-families [mission_board mission_workflow mission_workstation mission_context mission_memory mission_universe mission_ops mission_router mission_tool_directory]
    :directory-tool mission_tool_directory
    :max-primary-families 12
    :agent-tool-surface-governance
      ((principle
         :rule "Capability authority is shared; the preferred operator surface is agent-engine specific and must be projected by mission_tool_directory.")
       (surface-pair missiond
         :cli missiond-cli
         :mcp missiond-mcp
         :parity-command "missiond-cli tools list --json"
         :rule "missiond-cli is the shell-native Codex/operator adapter over the same daemon IPC tools as missiond-mcp; it must not copy business logic from MCP handlers.")
       (surface-pair xjp
         :cli xjp-cli
         :mcp xjp-mcp
         :parity-command "xjp mcp parity --json"
         :rule "xjp-cli and xjp-mcp are sibling XJP ops surfaces; parity gaps must be exposed instead of relying on operator memory.")
       (agent codex-cli
         :preferred-surfaces [missiond-cli xjp-cli]
         :fallback-surfaces [missiond-mcp xjp-mcp]
         :rule "Codex CLI workstations prefer shell-native CLI tools for MissionD/XJP operations; MCP remains compatibility and diagnostics fallback.")
       (agent claude-code
         :preferred-surfaces [missiond-mcp xjp-mcp]
         :fallback-surfaces [missiond-cli xjp-cli]
         :rule "ClaudeCode workstations prefer MCP schemas, tool descriptions, and session-local MCP context; CLI tools are gap-fill or diagnostics surfaces."))
    :metadata-required [tool_family primary_action tier danger_level intent_examples preferred_surface compatibility_tools]
    :agent-rule "When unsure, call mission_tool_directory(action=\"recommend\", intent=...) before selecting a lower-level MCP tool. Tool families are a selection/readability layer; compatibility tools remain stable for existing workers."
    :invariants
      ["mission_tool_directory MUST expose list/recommend/lookup/explain/deprecated/guide actions over the primary tool-family catalog and remain read-only."
       "mission_agent_navigation owns catalog/review/feedback/suggest_entries; feedback may append only .missiond/v3/runtime/agent-navigation-review.json and must not mutate Board, KB, project registries, SSOT shards, or sibling repositories."
       "Public tools MAY remain numerous, but every high-frequency tool must map to a primary family and preferred surface."
       "mission_tool_directory(agent_engine=codex-cli) MUST return CLI-first MissionD/XJP surfaces; mission_tool_directory(agent_engine=claude-code) MUST return MCP-first MissionD/XJP surfaces."
       "Deprecated/raw tools MUST return a preferredFamily/preferredSurface hint instead of relying on operator memory."
       "MCP tool-family guide semantics must be read-only; the only allowed navigation write is mission_agent_navigation feedback appending its review sidecar."])

  (memory-provider-contract
    :schema "missiond.memory-provider.v1"
    :purpose "Make memory pluggable so MissionD Core can be open-sourced, multi-tenant, and multi-universe without carrying private conversation, KB, skill-evidence, embedding, or review-overlay data."
    :core-boundary "MissionD Core owns provider registry, scope resolution, query/write facades, context injection policy, and MCP compatibility; providers own memory data and retrieval internals."
    :scope-fields [tenant_id universe_id project_id user_id source_type source_id authority privacy_class review_state]
    :default-provider null-memory
    :providers
      ((provider null-memory
         :kind disabled
         :use-case open-source-default
         :capabilities []
         :rule "Open-source/default MissionD can run without private memory data; queries return explicit MEMORY_PROVIDER_DISABLED diagnostics.")
       (provider local-postgres-memory
         :kind local-postgres
         :use-case single-user-dev-compatible
         :capabilities [query remember review-overlay evidence-search evidence-promotion evidence-backfill conversation-ingest skill-evidence export purge]
         :data-owner "local MissionD database compatibility tables"
         :rule "Current MissionD KB/conversation tables are a compatibility provider implementation, not the permanent MissionD Core memory model.")
       (provider xjp-memory
         :kind remote-service
         :use-case private-multi-universe
         :capabilities [query remember review-overlay evidence-search evidence-promotion evidence-backfill conversation-ingest skill-evidence fts embedding rerank context-pack export purge]
         :runtime-env [MISSIOND_MEMORY_PROVIDER_URL MISSIOND_MEMORY_PROVIDER_TOKEN]
         :embedding-provider xjp-router
         :rerank-provider xjp-router
         :rule "Private deployments use xjp-memory for tenant/universe/project/user scoped memory, conversation history, skill evidence, embedding, rerank, and review overlay. Secrets and provider tokens stay in secret-store/env, never in Lisp."))
    :functions
      ((function memory-provider-registry
         :entry [V3-compiled-runtime env-config mission_memory.provider_status]
         :core ((step s1 :logic "load provider declarations and active provider selection from MISSIOND_MEMORY_PROVIDER_URL / MISSIOND_MEMORY_PROVIDER_MODE")
                (step s2 :logic "validate provider capabilities against requested operation")
                (step s3 :logic "return a configuration snapshot by default; use bounded direct HTTP loopback transport for xjp-memory provider calls and only probe /v1/memory/provider_status when probe=true; otherwise return explicit null/local compatibility diagnostics"))
         :egress [MemoryProviderConfig provider-status])
       (function memory-scope-resolution
         :entry [BoardTask project-registry user-request active-universe]
         :core ((step s1 :logic "resolve tenant/universe/project/user scope before every memory query or write")
                (step s2 :logic "reject unscoped global memory reads unless workflow explicitly asks for cross-universe audit")
                (step s3 :logic "attach scope fields to provider requests and task-result artifacts"))
         :egress [memory-scope provider-namespace])
       (function memory-query-contract
         :entry [mission_memory.query mission_kb_query context-pack-builder]
         :core ((step s1 :logic "apply memory-scope-resolution")
                (step s2 :logic "apply review overlay and default active-only retrieval")
                (step s3 :logic "call provider query with explicit capability and privacy class")
                (step s4 :logic "return lane-labeled evidence without injecting broad KB into prompts by default"))
         :egress [memory-query-result context-evidence-lane])
       (function memory-write-contract
         :entry [mission_memory.remember mission_kb_remember intent-memory-capture memory-review-batch-runner]
         :core ((step s1 :logic "require explicit scope and write reason")
                (step s2 :logic "route high-confidence stable intent to provider remember")
                (step s3 :logic "route uncertain or conflicting items to review overlay/candidate artifacts")
                (step s4 :logic "preserve source_refs and supersession metadata"))
         :egress [memory-record review-candidate])
       (function memory-review-overlay-contract
         :entry [mission_memory.review mission_kb_review memory-review-batch-runner]
         :core ((step s1 :logic "write non-destructive review overlay")
                (step s2 :logic "exclude superseded/historical/duplicate/stale/delete-candidate/needs-human from default retrieval")
                (step s3 :logic "keep original evidence available with include_archived=true or state_filter"))
         :egress [review-overlay-state])
       (function memory-evidence-search-contract
         :entry [mission_memory.evidence_search mission_context_gather mission_repo_search]
         :core ((step s1 :logic "resolve scope and profile before retrieval")
                (step s2 :logic "filter allowed evidence lanes before FTS/vector/rerank")
                (step s3 :logic "return compact EvidenceItem projections with provenance and raw_policy")
                (step s4 :logic "MissionD authority evidence lanes are served from local evidence_items even when xjp-memory is configured as the remote memory provider")
                (step s5 :logic "dedupe repeated context_gather/backfill projections by lane/source/project before returning search results"))
         :egress [evidence_items])
       (function memory-evidence-promotion-contract
         :entry [mission_memory.evidence_promote memory-review-batch-runner]
         :core ((step s1 :logic "load compact evidence item by id")
                (step s2 :logic "reject runtime_truth/project_ssot promotion because those lanes are already authoritative")
                (step s3 :logic "require TTL/version/release/commit bound for deploy/config/dependency facts")
                (step s4 :logic "write KB only through remember plus active knowledge_review_state evidence overlay"))
         :egress [knowledge knowledge_review_state])
       (function memory-evidence-backfill-contract
         :entry [mission_memory.evidence_backfill maintenance-worker]
         :core ((step s1 :logic "summarize raw conversations into conversation_episodes/conversation_fact_extracts without deleting conversation_messages")
                (step s2 :logic "source=projects/support/all prewarms local compact evidence_items from compiled_project_universe and compiled service support catalogs without raw conversation scans, even when xjp-memory is the configured remote provider")
                (step s3 :logic "index skill/support evidence through compact evidence_items and skill_evidence_items projections")
                (step s4 :logic "credential_refs are counted only unless include_credentials=true; secret values are never indexed")
                (step s5 :logic "mark conversation/skill derived facts needs_review until explicit promotion")
                (step s6 :logic "isolate large async context-gather futures from project/support authority backfill so compiled authority prewarm cannot stack-overflow daemon worker threads")
                (step s7 :logic "backfill evidence_refs stay compact, keeping service/support key fields instead of whole runtime payloads"))
         :egress [conversation_episodes conversation_fact_extracts skill_evidence_items evidence_items])
       (function memory-context-injection-policy
         :entry [resident-master context-pack-builder worker-brief]
         :core ((step s1 :logic "default to no KB prefetch")
                (step s2 :logic "inject memory only when workflow declares memory scope and evidence purpose")
                (step s3 :logic "include provider/source/scope labels so agents can distinguish long-term memory from SSOT and runtime evidence"))
         :egress [context-pack-memory-lane]))
    :invariants
      ["MissionD Core MUST NOT require private memory data to boot, run Board/workstation workflows, or pass open-source checks."
       "Every memory query/write MUST resolve tenant/universe/project/user scope before calling a provider."
       "mission_kb_query and mission_kb_remember are compatibility leaves; preferred agents use mission_memory query/remember/review/provider_status."
       "Provider implementations own FTS, embedding, rerank, conversation archive, skill evidence index, active memory, archive state, export, and purge."
       "mission_memory.provider_status MUST NOT require remote HTTP by default; remote health probing is explicit with probe=true."
       "Default context-pack generation MUST NOT preload KB/history/provider logs; memory is opt-in by workflow and scope."]
    :checker "node scripts/check-v3-service-extraction-isomorphism.mjs")

  (context-surface-registry
    :schema "missiond.context-surface-registry.v1"
    :purpose "Single registry for MissionD context-like artifacts so new context packages do not copy evidence gathering, startup protocol, or worker sidecar semantics under a new schema name."
    :canonical-runtime-builder mission_context_gather
    :canonical-startup-surface mission_context_boot
    :canonical-planning-ledger context-pack
    :registry-owner memory-knowledge-runtime
    :surface-classes
      ((class planning-ledger
         :role "Append-only human/agent planning evidence that may later compile into accepted shards."
         :may-gather-evidence false)
       (class runtime-gather
         :role "Profile-first source aggregation over SSOT/project/memory/board/infra/conversation lanes."
         :may-gather-evidence true)
       (class derived-binding
         :role "Hashable capsule or file derived from a context gather result and bound to an interaction, conversation, BoardTask, or worker."
         :may-gather-evidence false)
       (class startup-contract
         :role "Small collaboration protocol loaded before task-specific evidence."
         :may-gather-evidence false)
       (class worker-sidecar
         :role "A bounded task/worker projection that points at an existing context source and scope."
         :may-gather-evidence false)
       (class support-context
         :role "Typed sub-payload used inside an owning surface; it is not a standalone context package."
         :may-gather-evidence false))
    :surfaces
      ((surface planning-context-pack
         :schema "missiond.context-pack.v1"
         :class planning-ledger
         :authority ssot
         :producer [scripts/context-pack-append.mjs]
         :consumers [scripts/context-pack-compile-shards.mjs scripts/context-pack-materialize-wave.mjs scripts/context-pack-run-wave.mjs]
         :must-not [runtime-evidence-aggregation startup-protocol conversation-binding])
       (surface runtime-context-gather
         :schema "missiond.context-gather.v1"
         :artifact-schema "missiond.context-gather-artifact.v1"
         :class runtime-gather
         :authority canonical-runtime-context-builder
         :producer mission_context_gather
         :consumers [mission_interaction Jarvis mission_task_delegate resident-master Codex-App-bootstrap]
         :owns [evidence-lanes source-profile-policy raw-source-policy support-catalog materialized-context-pack-file context-gather-runs-read-model])
       (surface context-navigation-profile
         :schema "missiond.context-navigation-profile.v1"
         :class support-context
         :authority runtime-context-gather-subpayload
         :producer mission_context_gather
         :source [query source_profile source_summaries support_catalog]
         :rule "Profile-aware evidence navigation sub-payload with selected_profiles, rejected_profiles, known_surfaces, required_questions, next_reads, verification_plan, evidence_status, and risk_flags. It guides agents toward bounded evidence collection but does not replace SSOT, evidence_lanes, or runtime closure authority.")
       (surface interaction-context-capsule
         :schema "missiond.context-capsule.v1"
         :class derived-binding
         :authority derived-from-context-gather
         :producer context_capsule::generate_lisp_capsule
         :source mission_context_gather
         :binding-targets [conversation topic BoardTask intent_alignment plan task_result_artifact]
         :must-not [perform-retrieval copy-source-profile-policy])
       (surface codex-boot-context
         :schema "missiond.codex-boot-context.v1"
         :policy-schema "missiond.codex-boot-context-policy.v1"
         :class startup-contract
         :authority boot-protocol
         :producer mission_context_boot
         :source ".missiond/v3/evidence/codex-boot-context.lisp"
         :must-not [bulk-chat-history raw-provider-logs unreviewed-kb-dump])
       (surface codex-app-bootstrap-hints
         :schema "missiond.codex-app-context-pack.v1"
         :live-schema "missiond.codex-app-live-context.v1"
         :class startup-contract
         :authority fallback-hints-only
         :producer "scripts/mission-context-pack.mjs"
         :source [mission_context_boot mission_context_gather]
         :rule "When MissionD runtime/MCP is available this script MUST delegate to mission_context_boot and mission_context_gather by default; --offline is deterministic fallback only. The pack MUST expose required_tool_sequence plus navigation_profile.recommended_tool_sequence, and those sequences MUST route compact evidence through mission_context_gather, mission_memory evidence_search, and mission_repo_search before ad-hoc shell rg.")
       (surface swarm-context-pack
         :schema "missiond.swarm-context-pack.v1"
         :class worker-sidecar
         :authority worker-dispatch-sidecar
         :producer mission_swarm_run
         :source [context_pack_path grounding_context_id target_project_roots read_scope write_scope]
         :must-not [evidence-lane-aggregation replace-context-pack-v1])
       (surface master-control-context-pack
         :schema "missiond.master-control-context-pack.v1"
         :class worker-sidecar
         :authority resident-master-runtime-snapshot
         :producer resident-master-control
         :source [active-objective checkpoint event-summary context_pack_path]
         :rule "Master-control context packs carry runtime snapshot fields; protocol text should be sourced from codex-boot-context / this registry, not re-authored as an independent context policy.")
       (surface context-slice
         :schema "missiond.context-slice.v1"
         :class derived-binding
         :authority shared-memory-slice
         :producer mission_context_slice
         :source shared_memory)
       (surface context-atlas
         :schema "missiond.context-atlas.v1"
         :compat-schemas ["missiond.context-atlas.dispatch.v0" "missiond.context-atlas.v0"]
         :class support-context
         :authority task-brief-support)
       (surface interaction-media-context
         :schema "missiond.interaction-media-context.v1"
         :class support-context
         :authority interaction-gateway-subpayload)
       (surface permission-context
         :schema "missiond.permission-context.v1"
         :class support-context
         :authority auth-permission-subpayload)
       (surface conversation-analysis-context
         :schema "missiond.conversation.analysis_context.v1"
         :class support-context
         :authority conversation-query-subpayload)
       (surface deployment-events-context
         :schema "missiond.deployment-events-context.v1"
         :class support-context
         :authority context-gather-subpayload)
       (surface runtime-environment-context
         :schema "missiond.runtime-environment-context.v1"
         :class support-context
         :authority context-gather-subpayload)
       (surface context-gather-evidence-lanes
         :schema "missiond.context-gather-evidence-lanes.v1"
         :class support-context
         :authority context-gather-subpayload)
       (surface context-noise-diagnostics
         :schema "missiond.context-noise-diagnostics.v1"
         :class support-context
         :authority context-gather-subpayload)
       (surface context-noise-metrics
         :schema "missiond.context-noise-metrics.v1"
         :class support-context
         :authority context-gather-subpayload)
       (surface context-pack-run-wave
         :schema "missiond.context-pack-run-wave.v0"
         :class support-context
         :authority context-pack-runner-output))
    :invariants
      ["mission_context_gather is the only runtime surface allowed to aggregate evidence lanes, raw-source policy, support_catalog, and context_noise_metrics."
       "Context surfaces outside class runtime-gather MUST point at an existing source context, task scope, or support payload; they MUST NOT independently reimplement source-profile policy."
       "Codex App bootstrap context is fallback hints only: when MissionD runtime is reachable, it MUST call mission_context_boot / mission_context_gather rather than maintain an independent rule set."
       "New missiond.*context*.v* schemas must be declared in context-surface-registry with class, authority, producer, and source/consumer semantics before checker approval."
       "Master-control and swarm context packs are worker sidecars; they must converge toward consuming registry/boot/gather facts instead of embedding full prompt policy prose."]
    :checker "node scripts/check-v3-context-surface-registry.mjs --json")

  (evidence-lane-policy
    :schema "missiond.evidence-lane-policy.v1"
    :purpose "Typed evidence lanes govern MissionD retrieval, context injection, raw-source access, privacy, freshness, and promotion. Search is profile-first and filter-before-vector; source-group calls are compatibility adapters into compact EvidenceItem projections."
    :primary-read-model evidence_items
    :run-metrics-read-model context_gather_runs
    :profile-order [intent_default deploy_ops conversation_audit full_debug]
    (lane runtime_truth
      :authority-class runtime_truth
      :source-types [runtime_environment deploy_release_manifest deploy_center_provenance health_smoke runtime_status]
      :default-profiles [intent_default deploy_ops conversation_audit full_debug]
      :raw-policy compact_only
      :privacy-class operational
      :validity [current_rule]
      :freshness hot_runtime
      :injectable-by-default true
      :promotion-rules [already-authoritative no-kb-promotion])
    (lane project_ssot
      :authority-class file_first_lisp_and_compiled_project_universe
      :source-types [project_resolution project_registry ssot compiled_project_universe service_runtime_universe]
      :default-profiles [intent_default deploy_ops conversation_audit full_debug]
      :raw-policy compact_only
      :privacy-class internal
      :validity [current_rule project_specific]
      :freshness compiled_runtime_bound
      :injectable-by-default true
      :promotion-rules [already-authoritative no-conversation-override])
    (lane reviewed_kb
      :authority-class knowledge_review_state
      :source-types [knowledge knowledge_review_state promoted_decision incident_pattern active_fact]
      :default-profiles [intent_default deploy_ops conversation_audit full_debug]
      :raw-policy compact_only
      :privacy-class internal
      :validity [active_fact decision incident_pattern]
      :freshness ttl_or_version_bound
      :injectable-by-default true
      :promotion-rules [review-required ttl-required-for-deploy-config-dependency])
    (lane active_board
      :authority-class board_projection
      :source-types [board_task workflow_run incident deploy_work_order task_result_projection]
      :default-profiles [intent_default deploy_ops conversation_audit full_debug]
      :raw-policy compact_only
      :privacy-class internal
      :validity [current_state active_work_order]
      :freshness active_task_bound
      :injectable-by-default true
      :promotion-rules [artifact-before-kb])
    (lane skill_evidence
      :authority-class evidence_only
      :source-types [skill_metadata skill_procedure skill_operational_fact skill_warning skill_credential_ref infra_evidence]
      :default-profiles [deploy_ops full_debug]
      :raw-policy compact_only
      :privacy-class internal
      :validity [current_rule historical_pattern deprecated project_specific evidence_only]
      :freshness version_bound_or_historical
      :injectable-by-default false
      :promotion-rules [needs_review-before-kb operational-fact-samples-high-confidence-only])
    (lane conversation_audit
      :authority-class provider_durable_conversation_read_model
      :source-types [conversation_episode conversation_fact_extract conversation_duplicate_group conversation_message_raw]
      :default-profiles [conversation_audit full_debug]
      :raw-policy raw_opt_in_only
      :privacy-class audit
      :validity [derived_from_conversation historical_pattern needs_review]
      :freshness time_range_bound
      :injectable-by-default false
      :promotion-rules [episode-first raw-message-audit-only fact-extract-needs-review])
    (lane cold_archive
      :authority-class forensics_only_cold_archive
      :source-types [archived_session true_user_utterance transcript_dump research_dump raw_provider_log old_compiled_projection]
      :default-profiles [full_debug]
      :raw-policy explicit_path_or_full_debug_only
      :privacy-class audit
      :validity [historical_evidence stale duplicate superseded]
      :freshness cold_archive
      :injectable-by-default false
      :promotion-rules [never-default no-auto-promotion])
    (lane support_refs
      :authority-class redacted_support_catalog
      :source-types [support_catalog deployment_closure_policy deploy_center_service github_workflow service_manifest db_migration_namespace health_endpoint smoke_endpoint release_lease runtime_observation release_evidence closure_verdict agent_ref secret_ref]
      :default-profiles [intent_default deploy_ops conversation_audit full_debug]
      :raw-policy secret_refs_only
      :privacy-class reference
      :validity [current_reference project_specific]
      :freshness runtime_or_catalog_bound
      :injectable-by-default true
      :promotion-rules [secret-values-never-indexed provenance-required deploy-closure-verdict-required])
    :profiles
      ((profile intent_default
         :allowed-lanes [runtime_truth project_ssot reviewed_kb active_board support_refs]
         :raw-sources false
         :credential-values false)
       (profile deploy_ops
         :allowed-lanes [runtime_truth project_ssot reviewed_kb active_board support_refs skill_evidence]
         :raw-sources false
         :credential-values false
         :credential-refs true)
       (profile conversation_audit
         :allowed-lanes [runtime_truth project_ssot reviewed_kb active_board support_refs conversation_audit]
         :raw-sources false
         :raw-message-access explicit_opt_in)
       (profile full_debug
         :allowed-lanes [runtime_truth project_ssot reviewed_kb active_board support_refs skill_evidence conversation_audit cold_archive]
         :raw-sources true
         :credential-values false))
    :read-models
      ((table evidence_items
         :lane-field lane_id
         :fields [lane_id authority_class source_type source_id source_ref project_id task_id title summary validity privacy_class freshness score raw_policy evidence_refs metadata]
         :rule "All searchable compact evidence lands here; raw conversations, raw skills, and cold archives remain in their original stores.")
       (table context_gather_runs
         :fields [source_profile lane_counts filtered_hits raw_sources_included credential_opt_in conversation_opt_in resolver_source runtime_root_consistent artifact_hash diagnostics]
         :rule "Every mission_context_gather call with persist_read_model=true records lane counts, raw-source injection state, semantic filtering, credential opt-in, resolver source, and runtime/root consistency; persist=true additionally binds the artifact_hash.")
       (table conversation_episodes
         :rule "Reusable conversation summaries are searchable; conversation_messages remains the raw audit layer.")
       (table conversation_fact_extracts
         :rule "Facts derived from conversations are tagged derived_from_conversation and needs_review until promoted.")
       (table skill_evidence_items
         :rule "Skills split into metadata, procedure, operational_fact, warning, and credential_ref items with validity states."))
    :invariants
      ["mission_context_gather MUST resolve project/service before FTS/vector search and MUST apply profile lane allowlists before any semantic retrieval."
       "Default worker context MUST inject compact evidence_items/support_catalog/evidence_lanes only; raw source payloads are excluded unless include_raw_sources=true or full_debug."
       "Default worker context compact support_catalog MUST omit the full deployment_closure payload unless deploy_ops/full_debug, include_raw_sources=true, explicit infra target, or deployment-closure query anchors opt in."
       "conversation_message_raw and cold_archive data MUST NOT enter intent_default or deploy_ops retrieval. conversation_audit may use bounded episode/fact extracts; raw messages require explicit opt-in."
       "skill_evidence is evidence-only unless a review/promotion workflow promotes an item into reviewed_kb with TTL or version bounds for deploy/config/dependency facts."
       "support_refs MUST expose secret_ref namespace/key/provenance/availability only; secret values are never indexed, embedded, or injected."
       "mission_repo_search MUST normalize repository text hits into lane-tagged compact evidence references and MUST NOT return cold_archive/conversation_audit hits unless the source_profile allows those lanes."
       "mission_context_gather persist_read_model defaults true so compact context_gather_runs/evidence_items projections are available to the next search without requiring worker artifact creation."
       "context_gather_runs MUST persist lane counts, raw source inclusion, conversation cross-project drops when available, filtered semantic hit counts, credential opt-in, low-confidence skill evidence drops when available, resolver source, and runtime/root consistency."]
    :checker "node scripts/check-v3-memory-kb-isomorphism.mjs")

  (memory-kb-policy
    :desc "Lisp-owned memory extraction budget for the memory-kb surface."
    :pending-message-limit 60
    :tool-result-preview-chars 1000
    :assistant-preview-chars 500
    :active-memory-target-ratio 0.10
    :sensitive-query-suppression [architecture:module]
    :review-states [active superseded-by-lisp superseded-by-code historical-evidence duplicate wrong-or-stale delete-candidate needs-human]
    :default-query-policy "exclude current review states superseded-by-lisp/superseded-by-code/historical-evidence/duplicate/wrong-or-stale/delete-candidate/needs-human unless include_archived or state_filter is explicit"
    :invariants
      ["mission_memory_pending MUST project batch size and preview truncation lengths from memory-kb-policy."
       "mission_memory_pending MUST cache the served realtime extraction batch for the active extraction cycle and allow bounded replay after context compaction; if replay cache is missing or exhausted it MUST return structured MEMORY_PENDING_ALREADY_SERVED rather than a successful empty result."
       "mission_memory_pending MUST classify deployment-monitor, runtime-report, worker-instruction, and provider-preamble text noise into input skip diagnostics before active memory extraction; deployment-monitor covers deploy/build/smoke/rollback/agent-update/provenance diagnostics plus deployment-event-response, xjp_build_wait, xjp_deploy_watch, and xjp_deploy_status monitor text; user utterances MUST never be filtered by these text classifiers."
       "mission_kb_query MUST suppress architecture:module details for sensitive credential/secret/SSH/token queries unless the caller explicitly scopes category/project to that architecture surface."
       "mission_kb_query MUST support excludeCategory / exclude_category for explicit category suppression, including subcategory matches such as memory excluding memory:*."
       "mission_kb_mutate(action=batch_remember) MUST accept a bounded entries array so memory review and distillation workflows do not need to spam one MCP call per KB row."
       "mission_kb_remember MUST pass through one shared dedupe gate in KbStore::kb_remember before any realtime/deep-analysis/manual pipeline can create a new active key; same source-session duplicates use a stricter low threshold and merge evidence_refs/source_sessions/superseded_by instead of overwriting them."
       "mission_kb_review MUST write a non-destructive knowledge_review_state overlay; it MUST NOT mutate or delete the original knowledge row."
       "Low-confidence semantic duplicate candidates MUST create a needs-human knowledge_review_state artifact and leave the raw row as evidence rather than deleting or silently activating it."
       "Large KB cleanup MUST calibrate with at least five manual batches before batch overlay application; target active memory is about 10%, with needs-human hidden from default retrieval."
       "mission_kb_query default retrieval MUST honor the review overlay while include_archived=true and state_filter preserve audit access to historical evidence."
       "A real MissionD project with .missiond but no memory-kb-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (learning-engine-policy
    :desc "Lisp-owned autonomous learning engine cadence, pty budget, and low-utility reflection policy."
    :realtime-extraction-timeout-secs 300
    :realtime-empty-backoff-base-secs 30
    :realtime-empty-backoff-max-secs 900
    :deep-analysis-zero-output-fuse-threshold 3
    :deep-analysis-zero-output-fuse-secs 3600
    :decision-tier3-timeout-secs 300
    :habit-scan-timeout-secs 600
    :token-spend-guard-window-secs 3600
    :token-spend-guard-soft-limit 250000
    :timeline-analysis-interval-secs 43200
    :timeline-analysis-window-hours 12
    :timeline-error-limit 20
    :timeline-llm-sample-limit 50
    :timeline-slow-event-limit 20
    :timeline-slow-threshold-ms 60000
    :idle-explore-interval-secs 7200
    :habit-scan-interval-secs 14400
    :habit-scan-batch-size 5
    :kb-auto-gc-interval-secs 3600
    :kb-consolidation-interval-secs 86400
    :kb-reflection-interval-secs 604800
    :kb-reflection-utility-threshold 0.3
    :kb-reflection-min-access 3
    :kb-reflection-max-entries 20
    :kb-reflection-max-tokens 2000
    :decision-harvest-interval-secs 86400
    :cooccurrence-refresh-interval-secs 21600
    :invariants
      ["LearningEngineRuntimeConfig MUST load learning-engine-policy from .missiond/v3/missiond-blueprint.lisp and fail with V3_BLUEPRINT_CONFIG_ERROR for real MissionD projects whose V3 blueprint or policy block is missing."
       "Realtime extraction, Tier3 decision escalation, and historical habit scan pty.send budgets MUST project from learning-engine-policy."
       "Realtime extraction MUST apply exponential empty-queue backoff from learning-engine-policy after consecutive no-user-work probes, and reset the backoff as soon as a real batch is dispatched."
       "Deep analysis MUST apply a Lisp-projected zero-output saturation fuse after consecutive completed deep-analysis jobs produce no KB mutations; while fused it MUST skip dispatch and expose diagnostics."
       "Memory/learning workers MUST consult token_usage_ledger through a Lisp-projected sliding-window token-spend soft guard before dispatch; if the window crosses token-spend-guard-soft-limit, MissionD MUST pause the memory domain through ControlTree and emit diagnostics instead of spending into a provider quota cliff."
       "Realtime extraction MUST claim the extraction lane before running pending-message DB probes; pending realtime SQL MUST use EXISTS/LATERAL LIMIT or bounded materialized-candidate shapes instead of global COUNT(DISTINCT)/ROW_NUMBER scans; deep-analysis active-conversation probes MUST use bounded EXISTS/OFFSET checks instead of full message COUNT scans so repeated ticks or status refreshes cannot exhaust the Postgres pool."
       "Memory extraction pending selectors MUST filter MissionD self-referential worker slots, including slot-memory*, slot-diagnosis*, and agent-* sessions, even when historical role attribution mistakenly labeled them as user conversations."
       "Learning maintenance cadences (timeline analysis, idle exploration, habit scan, KB auto-GC, KB consolidation, KB reflection, decision harvest, co-occurrence refresh) MUST project from learning-engine-policy."
       "Timeline analysis read windows, event limits, and slow-request threshold MUST project from learning-engine-policy."
       "KB reflection low-utility threshold, minimum access count, max entries, and max_tokens MUST project from learning-engine-policy."
       "Timeline projection SQL MUST cast string-bound since/until parameters as ::timestamptz when comparing against event_log.ts so PG never raises 'operator does not exist: timestamp with time zone >= text' from Timeline Analyst, mission_timeline, or stratified queries."
       "Timeline Analyst MUST check the Gemini provider gate before collecting timeline evidence or calling Gemini; when the gate is closed it MUST advance the cadence marker and skip without warning spam or repeated LLM attempts."
       "Timeline Analyst MUST advance its cadence marker on provider/config/upstream failures as well as success, so missing LLM credentials or transient provider errors cannot retry every learning tick and pollute runtime logs."])

  (conversation-ingestion-policy
    :desc "Lisp-owned read-model window and limit defaults for conversation, event, and timeline query surfaces."
    :conversation-get-tail-default 50
    :conversation-search-default-limit 10
    :message-search-default-limit 20
    :analysis-context-max-turns 50
    :label-calibration-sample-limit 200
    :jarvis-stream-envelope-schema "missiond.jarvis-stream-envelope.v1"
    :context-before-default 3
    :context-after-default 5
    :conversation-events-default-limit 100
    :agent-trajectory-default-limit 200
    :timeline-query-default-limit 50
    :timeline-query-max-limit 200
    :timeline-search-default-limit 20
    :timeline-search-max-limit 100
    :intent-router-model "claude-opus-4.6"
    :intent-router-timeout-ms 10000
    :vision-codex-binary "codex"
    :vision-codex-model "gpt-5.4"
    :vision-codex-idle-timeout-secs 120
    :vision-codex-absolute-timeout-secs 300
    :invariants
      ["mission_conversation_get/search/message_search/context_around MUST project default limits from conversation-ingestion-policy."
       "mission_conversation_events and mission_agent_trajectory MUST project default limits from conversation-ingestion-policy."
       "mission_timeline query/search MUST project default and max limits from conversation-ingestion-policy."
       "mission_timeline(action=wait) MUST expose bounded EventBus waits for board/slot/task/system predicates; timeout/lag returns diagnostic JSON so polling remains only an explicit fallback."
       "Explicit opt-in UserPromptSubmit context prefetch intent router model and timeout MUST project from conversation-ingestion-policy instead of local claude-opus/10000ms literals; default workstation hook sync removes UserPromptSubmit prefetch until a memory-audit workflow enables it."
       "Codex vision worker binary/model/idle timeout and CodexCli absolute timeout MUST project from conversation-ingestion-policy instead of local gpt-5.4/120s/300s literals."
       "Historical conversation event/tool-call backfills MUST NOT run unconditionally on daemon startup; they are opt-in maintenance/workflow operations gated by llm.yaml backfill_enabled or MISSIOND_CONVERSATION_BACKFILL_ON_STARTUP=1 so daemon restarts do not replay large provider histories as foreground CPU load."
       "llm_summary/topic embedding generation MUST default to human/Jarvis/direct CLI chat read models only; worker/meta/memory-slot conversations project their canonical result through task-result-artifact so skill-injection prompts, quota diagnostics, and worker instructions do not pollute user-facing conversation summaries."
       "mission_conversation_query search MUST collapse same-project near-identical session hits by default using response-layer duplicate diagnostics, while includeDuplicateSessions=true keeps folded session metadata available and collapseSimilar=false preserves uncollapsed diagnostic behavior."
       "includeDuplicateSessions=true MUST expose compact audit metadata for folded sessions (sessionId, project/canonical project, conversation type, status, startedAt, message count, slot, compact matchReason, rawMatchReasonChars) without rawMatchReason unless includeRawMatchReason=true is also explicit."
       "Conversation analysis_context MUST be a bounded read model: it samples at most analysis-context-max-turns from calibrated turns and never pulls raw worker/provider chatter into user-intent inference."
       "Conversation label calibration MUST remain overlay-first: message_labels stores speaker/origin/canonical_state evidence, rawRole is preserved, and calibration reports are reviewed before any destructive rewrite."
       "Jarvis SSE and OpenAI-compatible chat surfaces MUST emit jarvis-stream-envelope-schema frames with conversation_id/task_id correlation, process affinity, and semantic event kind; PTY status is diagnostic and cannot replace the envelope."
       "Jarvis mobile/public clients MUST call /api/readiness after /health so daemon liveness, default slot busy state, and slot-unavailable startup failures are distinct operator-visible states; /health alone MUST NOT be presented as end-to-end readiness."
       "Jarvis mobile/public clients and operators MUST have /api/monitor/jarvis as the chain monitor for proxy reachability, daemon release, default slot readiness, MCP config, PTY live-screen/log evidence, and compiled runtime config; readiness is UX state, monitor is debug evidence. Post-deploy automation may call localhost-only /internal/jarvis/slot/ensure to restore an Exited/Error default slot before monitor smoke, but the public monitor remains read-only."
       "Jarvis chat surfaces MUST write provider usage into token_usage_ledger with slot/task/message linkage so billing and quota views read one source of truth."
       "A real MissionD project with .missiond but no conversation-ingestion-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (evidence-governance-policy
    :desc "Unified evidence model for Memory/KB, Logs, Timeline, Conversation, and worker outputs."
    :authority-order [task_result_artifacts provider_durable_conversation event_log knowledge_review_state board_projection]
    :roles
      ((task-result-artifact :role canonical-worker-output :rule "Worker and workflow finals land here first; Board notes are projections.")
       (conversation :role provider-user-turn-read-model :rule "Conversation rows/messages preserve provider/user turns for audit and retrieval, not completion authority.")
       (timeline :role event-causality-view :rule "event_log / EventBus projections explain when and why something happened.")
       (kb-memory :role reviewed-long-term-knowledge :rule "KB is curated active memory after review overlay; raw historical logs are not active knowledge.")
       (board :role coordination-projection :rule "BoardTask state coordinates work and operator decisions; it is not the canonical worker result body."))
    :runtime-projection [mission_shared_memory.evidence_view task_result_artifacts conversations event_log knowledge_review_state board_tasks]
    :invariants
      ["mission_shared_memory(action=evidence_view) MUST return the unified evidence governance view for a task/project, grouping task_result_artifacts, conversations, event_log/shared_events, KB review overlay, and Board projection into named evidence lanes."
       "Memory/KB, Logs, Timeline, and Conversation MUST NOT each invent their own final-result authority; worker outputs use task-result-artifact, conversations are read models, timeline is causality, KB is reviewed long-term knowledge, and Board is coordination projection."
       "Default agent context may cite the evidence view lanes, but must not treat raw PTY, raw provider transcript, or unreviewed KB as higher authority than task-result-artifacts and durable events."])

  (cli-conversation-ingestion
    :desc "Canonical CLI conversation-log ingestion contract for ClaudeCode, Gemini CLI, and Codex CLI."
    :legacy-aliases ["claude_cli" "pty_jsonl"]
    (source claude-code
      :canonical "claude_code"
      :paths ["~/.claude/projects/**/*.jsonl" "~/.claude/history.jsonl"]
      :watcher "crates/missiond-core/src/cc_tasks/watcher.rs"
      :route "crates/missiond-daemon/src/infra/ingestion_router.rs"
      :history-import "scripts/import-claude-history-jsonl.mjs"
      :normalizer "scripts/normalize-claudecode-conversations.mjs"
      :audit "scripts/audit-claudecode-conversations.mjs")
    (source gemini-cli
      :canonical "gemini_cli"
      :paths ["~/.gemini/tmp/*/chats/*.json" "~/.gemini/tmp/*/chats/*.jsonl"]
      :watcher "crates/missiond-core/src/gemini_cli/watcher.rs"
      :route "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
      :audit "scripts/audit-gemini-conversations.mjs")
    (source codex-cli
      :canonical "codex_cli"
      :paths ["~/.codex/state_5.sqlite" "~/.codex/sessions/**/*.jsonl" "~/.codex/archived_sessions/*.jsonl" "~/.codex/session_index.jsonl" "~/.codex/history.jsonl"]
      :worker "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs")
    :invariants
      ["Conversation sources MUST be canonicalized before DB write: claude_code, gemini_cli, or codex_cli."
       "Legacy claude_cli and PTY transport pty_jsonl remain read aliases only; new non-transport source fields MUST name the canonical CLI."
       "mission_pty_status and mission_slots observability MUST be joinable with the latest conversation row by slot/session id and source."
       "mission_slots MUST reject or flag slot_sessions whose conversation source disagrees with the slot engine; stale provider drift must never masquerade as current state."
       "Codex CLI slot_sessions may contain a PTY placeholder id; mission_slots MUST fall back to the latest real codex_cli conversation for the slot project instead of surfacing a messageCount=0 placeholder as the latest durable conversation."
       "Codex CLI ingestion MUST scan the full state_5.sqlite thread set, including archived threads, and mark archived threads as historical status instead of dropping them from MissionD history."
       "Codex CLI ingestion MUST also discover raw rollout JSONL under ~/.codex/sessions/**/*.jsonl and ~/.codex/archived_sessions/*.jsonl even when state_5.sqlite has no thread row; session_meta.payload.id is the canonical conversation id, and raw-only imported rows MUST be recorded in conversation_source_state as provider-index-missing instead of being silently ignored."
       "Codex CLI conversation_source_state MUST distinguish current, provider-index-missing, missing-stale, path-mismatch, archived, and pty-placeholder evidence so audits can explain whether MissionD is missing provider history, whether the Codex provider-local index lost a rollout row, or whether a visible PTY is only a diagnostic placeholder."
       "Codex CLI runtime status MUST NOT treat archived=false in state_5.sqlite as proof that a conversation is actively running; provider source archive state, slot binding, durable final, and PTY state are separate evidence lanes."
       "Gemini request-log persistence MUST only consume Gemini provider LlmEvent variants; Codex CLI durable history belongs to codex_cli conversations/source-state, and non-Gemini LlmEvent replay MUST NOT pollute gemini_requests or generate duplicate insert warnings."
       "Codex CLI message ingestion MUST generate deterministic non-null message_uuid values from thread id, JSONL line number, role, and source event hash so reconcile/backfill cannot repeatedly insert duplicate NULL-uuid rows."
       "Codex CLI background ingestion MUST persist rollout size/mtime/line/complete watermarks and parse large rollout files in bounded pages after the last durable cursor; a 50k safety limit is per poll page, never a permanent history truncation."
       "When deterministic UUID ingestion meets an older NULL-uuid row with the same session, role, timestamp, and content, the DB layer MUST adopt that existing row by setting message_uuid instead of inserting a new duplicate row."
       "mission_conversation_get MUST defensively coalesce duplicate rows by message_uuid or role/timestamp/content fallback so frontend logs stay readable until historical cleanup is reviewed."
       "mission_conversation_get MUST retrieve tail messages with the indexed (session_id,id) path and assign display seq after duplicate coalescing; it MUST NOT use a ROW_NUMBER window over an entire large Codex/Gemini session."
       "Historical duplicate cleanup is dry-run/report-first; destructive DB cleanup must keep the earliest row in each duplicate group and require an explicit reviewed apply path."
       "Gemini background reconcile MUST use size/mtime companion watermarks to skip already-reconciled old chat files without reparsing full historical transcripts; manual reconcile may force a full scan."
       "Gemini manual/full reconcile MUST ignore count watermarks and replay raw ~/.gemini/tmp/*/chats/session-* files from message index 0 through deterministic message_uuid upserts, so historical sessions anchored before MissionD watcher startup can still be imported without duplicates."
       "Gemini manual/full reconcile MUST be reachable through mission_conversation_query(action=gemini_reconcile) / mission_conversation_gemini_reconcile, and that action MUST call gemini_reconcile_worker::run_gemini_reconciliation_now instead of relying on daemon restart or ad hoc SQL repair."
       "Gemini CLI tool lifecycle MUST close conversation_tool_calls with tool_result messages: parser emits tool_use/tool_result blocks, realtime ingestion and gemini_reconcile both persist has_tool_use/has_tool_result/content_types, and role=tool_result updates output_summary/raw_output/status rather than leaving tool calls pending."
       "Gemini CLI raw-vs-DB coverage MUST be auditable through scripts/audit-gemini-conversations.mjs: raw sessions missing in DB, DB conversations missing raw file, pending tool calls, and raw-vs-DB tool counts are reported before memory distillation trusts Gemini history."
       "Cursor/watermark advancement MUST happen after durable DB write acknowledgement, never before."
       "ClaudeCode ~/.claude/history.jsonl is a prompt-only historical source: import it as conversation_type=history_prompt, chat_type=history_jsonl, source=claude_code, speaker=human_user, authority=claude_history_prompt, and deterministic message_uuid=claude-history:<sha>; it MUST NOT be mistaken for assistant/tool transcript coverage."
       "ClaudeCode historical import MUST refresh conversations.message_count from actual inserted conversation_messages after import because database triggers/upserts can otherwise leave placeholder counts that make Logs and exports report double messages."
       "ClaudeCode conversation normalization MUST maintain a non-destructive overlay: conversation_source_state records current/missing-stale/path-mismatch/raw-only-local-command/raw-only-provider-prompt/raw-only-uningested source evidence, message_labels canonical_state marks exact role/timestamp/content duplicates as equivalent-duplicate, raw_role_state distinguishes native/reconstructed/provider-derived/ambiguous, and no provider JSONL row is physically deleted by normalization."
       "Conversation message labeling MUST be centralized in the deterministic message_labeler worker: rule evidence is stored in message_label_evidence, message_labels remains a replayable compatibility projection, consumer_watermarks gate incremental progress after durable writes, and explicit mission_conversation_query(action=label_audit|label_backfill) surfaces replace ad hoc whole-database label scripts."
       "True-user utterance export MUST include ClaudeCode history_prompt rows and exclude equivalent-duplicate, worker/subagent/compaction, task-bound sessions, MissionD runtime prompts, provider context, terminal artifacts, and local-command artifacts; verification must fail if BoardTask/Swarm prompt signatures leak into the export."
       "ClaudeCode provider role normalization MUST be shared by realtime watcher, per-session reconcile, and daily reconcile paths: top-level raw_role=user inside automated slot sessions normalizes to worker_user, interactive Jarvis/user conversations remain user, sidechain progress remains agent_user/agent_assistant, and raw_role is preserved for audit."
       "Historical ClaudeCode role repair is dry-run/report-first through scripts/report-claude-role-attribution.mjs; first pass reports suspected system/user/agent_user drift and never mutates DB."
       "Provider-aware conversation_type classification MUST live behind crates/missiond-core/src/db/conversation_query.rs::classify_conversation_type so ClaudeCode, Codex CLI, and Gemini CLI workers share one rule set: slot-bound sessions (any provider) classify as worker with durable slotId/taskId linkage; background-ingested Codex threads classify as codex_chat (parallel to gemini_chat), never as the human user fallthrough; real human Jarvis user sessions remain user."
       "ClaudeCode slot session capture and reconcile MUST bind conversations.task_id from the currently running BoardTask claimed by that slot after session UUID discovery and after lazy JSONL conversation creation, so mission_conversation_query(taskId=...) works while workers are still running rather than only after final evidence."
       "Codex CLI background ingestion MUST call classify_conversation_type AND preserve the provider role into raw_role so the conversation row carries enough metadata for audit_classification and the role-attribution report; the legacy hardcoded conversation_type=\"user\" + raw_role=None pattern is forbidden."
       "Codex CLI background ingestion MUST refresh conversation message_count from actual inserted rows after each import so the conversation list, Logs, and memory distillation samples do not regress to the initial upsert placeholder count."
       "Historical row classification repair is dry-run/report-first through db::conversation_query::audit_historical_classification: it returns HistoricalClassificationFinding values for codex_user_without_slot, codex_slot_not_worker, worker_loses_slot_linkage, codex_raw_role_missing, and claude_worker_prompt_signature; mission_conversation_query(action=audit_classification) reports candidates without mutation, mission_conversation_query(action=backfill_classification, apply=true) may apply only high-confidence repairs through set_conversation_type, backfill_missing_raw_roles_for_session for old Codex rows, and then rebuild conversation_turns via rebuild_session_turns."
       "Historical ClaudeCode message-role repair is also dry-run/report-first: mission_conversation_query(action=audit_message_roles) reports worker-session rows where source=claude_code, conversation_type=worker, role=user, raw_role=user, and content matches local-command or worker-prompt signatures; mission_conversation_query(action=backfill_message_roles, apply=true) may rewrite only those reviewed rows to worker_user and then rebuild conversation_turns via rebuild_session_turns. It MUST NOT delete provider messages or bulk-relabel real human Jarvis/user conversations."
       "Conversation turn repair is explicit and bounded: mission_conversation_query(action=turn_backfill, sessionId=...) clears only that session's conversation_turns and re-runs tagger_chunker on its canonical message stream; it does not rewrite raw provider logs."]
    :checker "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs")

  (upstream-pty-signatures
    :desc "Provider-aware PTY recognition signatures derived from upstream TUI source instead of screenshot-only heuristics."
    (provider codex-cli
      :canonical "codex_cli"
      :upstream "https://github.com/openai/codex"
      :ref "ff27d01676a93be7467b3893e82f41a7af7e1418"
      :source-paths ["codex-rs/tui/src/status_indicator_widget.rs"
                     "codex-rs/tui/src/chatwidget.rs"
                     "codex-rs/tui/src/bottom_pane/approval_overlay.rs"
                     "codex-rs/tui/src/bottom_pane/chat_composer.rs"]
      :signals [working-status esc-to-interrupt status-details approval-overlay composer-idle])
    (provider gemini-cli
      :canonical "gemini_cli"
      :upstream "https://github.com/google-gemini/gemini-cli"
      :ref "d9f273e44095b742e9ab74241e240c587ae27e64"
      :source-paths ["packages/cli/src/ui/types.ts"
                     "packages/cli/src/ui/components/LoadingIndicator.tsx"
                     "packages/cli/src/ui/components/InputPrompt.tsx"
                     "packages/cli/src/ui/components/messages/DenseToolMessage.tsx"]
      :signals [StreamingState.Idle StreamingState.Responding StreamingState.WaitingForConfirmation Thinking esc-to-cancel CoreToolCallStatus])
    (provider claude-code
      :canonical "claude_code"
      :upstream "/Users/jinchen/Downloads/claudecode/claudecode"
      :source-paths ["src/constants/spinnerVerbs.ts"
                     "src/constants/turnCompletionVerbs.ts"
                     "src/remote/sdkMessageAdapter.ts"
                     "src/cli/print.ts"]
      :signals [spinner-verbs turn-completion-verbs tool-progress auto-mode prompt-footer])
    :output PtyRecognitionSnapshot
    :states [running idle blocked complete unknown]
    :invariants
      ["Codex CLI and Gemini CLI MUST use provider-specific StateParser implementations and MUST NOT fall back to the ClaudeCode parser."
       "mission_pty_status MUST include PtyRecognitionSnapshot with provider, state, confidence, reason, phase/tool/blocked details when available."
       "Autopilot watchdogs MUST treat low-confidence unknown as diagnostic state rather than automatic BoardTask closure evidence."
       "If an upstream TUI signal changes, checker failure is preferred over silent downgrade to generic prompt heuristics."
       "recognize_screen MUST fuse SessionState with screen heuristics: an active processing SessionState (Thinking, Responding, ToolRunning, Confirming) MUST NOT be demoted to Blocked from screen_fallback confirmation or model-picker text; the fused snapshot is sourced from screen_fused active evidence or session_state, and explicit Confirming SessionState always preserves Blocked."
       "Confirming is an active turn state, not a TextOutput::Complete boundary: Codex/ClaudeCode/Gemini approval menus may block the provider turn, but completion can only fire after confirmation resolves and the real final answer returns to Idle/Exited."
       "Exited/terminal SessionState overrides stale running screen evidence; mission_pty_status and mission_slots MUST NOT expose recognition.state=running when the durable PTY session state is exited or error."
       "Codex MCP approval menus (`Allow the ... MCP server to run tool`, `Allow for this session`, `enter to submit | esc to cancel`) are explicit blocked TUI source signatures and MUST NOT be demoted to Running just because the SessionState is Thinking."
       "mission_pty_confirm MUST confirm option menus by human-like keyboard navigation (Down/Up then Enter), never by sending numeric shortcut keys; this applies to ClaudeCode, Codex CLI, and Gemini CLI."
       "recognize_claude_code Blocked MUST require explicit confirmation/model-picker UI (Enter to confirm, Do you want to proceed/make this edit/allow/use this api key, Select model, approval request); the bare words `approval` or `permission(s)` -- including the `bypass permissions on` composer-mode footer toggle and historical task-brief prose -- MUST NOT trigger Blocked on Idle or completed screens."
       "ClaudeCode worker MCP reconnect MUST follow `/mcp` -> Enter -> ArrowDown until missiond -> Enter -> Enter using arrow-key keystrokes only; numeric shortcut selection is forbidden because Claude Code's MCP picker numeric shortcuts have shifted between releases. The keystroke sequence is the SSOT and missiond-pty Session::mcp_reconnect_sequence MUST project from it."
       "When a ClaudeCode worker advertises supports_mcp=true but its mounted tool list does not include any mission_* tool after slot ready, master_control MUST file a durable claude_code_mcp_missing incident so the resident master is woken; if the /mcp arrow-key reconnect ritual does not surface mission_* tools within the policy budget, a follow-up claude_code_mcp_reconnect_failed incident is required, never a silent retry loop."]
    :checker "node scripts/check-v3-pty-recognition-isomorphism.mjs")

  (conversation-session-management
    :desc "Multi-tenant conversation session isolation, automatic topic threading, session-less entry, and context capsule binding."
    :schema "missiond.conversation-session-management.v1"

    (isolation-dimensions
      :desc "Every conversation session carries four Auth-derived isolation dimensions. Queries MUST filter by the caller's resolved isolation scope."
      :columns [user_id tenant_id application_id channel]
      :user_id    "Auth-resolved user identifier from xjp-auth OAuth2/OIDC; nullable for legacy CLI-only sessions."
      :tenant_id  "Auth-resolved tenant/organization; nullable for single-tenant local CLI."
      :application_id "Application context identifier (e.g. jarvis, cuthub, pcea); nullable for direct CLI."
      :channel    "Communication channel: cli, api, jarvis_sse, jarvis_mobile, openclaw, webhook; defaults to cli."
      :default-channel "cli"
      :migration "20260526000000_conversation_session_management.sql")

    (topic-threading
      :desc "Automatic topic splitting, association, and continuation within a tenant-scoped session stream."
      :columns [topic_id topic_label]
      :topic_id   "Deterministic topic thread identifier; conversations sharing a topic_id form a logical thread."
      :topic_label "LLM-generated human-readable topic label for display and search."
      :auto-split-rule "When context_gather detects semantic drift from the current topic (cosine similarity below topic_split_threshold), a new topic_id is minted and bound to the conversation."
      :continuation-rule "When a new interaction's query is semantically close to an existing topic (above topic_continue_threshold), the resolver binds the interaction to that topic_id for session continuation."
      :topic_split_threshold 0.35
      :topic_continue_threshold 0.70)

    (session-less-entry
      :desc "Clients are not required to create a new conversation before interacting. The system auto-resolves or creates sessions."
      :resolve-order [active-topic-match active-session-for-slot new-session]
      :rule "mission_context_gather and Jarvis chat surfaces MUST resolve an existing session by (user_id, tenant_id, application_id, channel, topic_id) before creating a new conversation row."
      :session-ttl-hours 24
      :max-active-sessions-per-user 10)

    (jarvis-mobile-history
      :desc "Authenticated Jarvis mobile/public HTTP clients restore durable conversation state from MissionD instead of local in-memory chat buffers."
      :read-model "missiond.jarvis-conversation-history.v1"
      :routes [GET:/api/jarvis/conversations GET:/api/jarvis/conversations/:conversation_id]
      :scope "Auth PermissionContext resolves user_id, tenant_id, application_id, channel; list/get MUST enforce this scope before returning messages."
      :write-rule "Jarvis interaction gateways MUST append both user messages and visible assistant final/diagnostic text to the same scoped jarvis_ui conversation."
      :legacy-rule "Legacy jarvis_ui rows missing Auth scope may be exposed only as explicit legacy_unscoped compatibility rows and MUST be backfilled to the caller scope when the caller continues that conversation.")

    (context-capsule
      :desc "Compressed Lisp context capsule generated per interaction from SSOT, KB, project registry, skill evidence, infra/deploy facts, and related history."
      :schema "missiond.context-capsule.v1"
      :columns [context_capsule_hash]
      :context_capsule_hash "SHA-256 hash linking to the materialized context-gather artifact in shared_memory."
      :binding-targets [intent_alignment plan board_task task_result_artifact]
      :generation-rule "Every grounded interaction MUST generate a context capsule via context_gather(persist=true) and bind the resulting hash to the conversation row and any associated BoardTask runtime_metadata."
      :capsule-format "lisp-sexp"
      :capsule-layers [L0-isolation L1-ssot-snapshot L2-active-kb L3-project-registry L4-skill-evidence L5-infra-facts L6-related-history L7-topic-context])

    :invariants
      ["conversations table MUST carry user_id, tenant_id, application_id, channel, topic_id, topic_label, context_capsule_hash columns after migration 20260526000000."
       "mission_conversation_query list/search actions MUST accept and enforce user_id, tenant_id, application_id, channel filters when provided."
       "Session-less entry resolution MUST prefer active topic matches over creating new sessions; only when no active topic match exists should a new conversation be created."
       "Context capsule generation MUST be idempotent: the same (query, isolation scope, timestamp window) produces the same capsule hash."
       "Topic auto-split MUST NOT fire for worker/meta/system conversation types; only user and codex_chat sessions participate in topic threading."
       "Context capsule binding to BoardTask MUST use runtime_metadata.context_capsule_hash, not a separate column, to avoid schema sprawl on the board_tasks table."
       "Conversation isolation filters MUST be additive: omitting a dimension returns all values for that dimension within the caller's permission scope."
       "Jarvis mobile/public HTTP history list/get MUST return only scoped jarvis_ui read-model rows plus explicitly labeled legacy_unscoped compatibility rows during migration."
       "Jarvis chat completions and interaction-envelope gateways MUST persist visible assistant completion text; PTY/provider logs alone are not sufficient mobile conversation history."
       "Legacy CLI sessions without Auth context MUST default to user_id=NULL, tenant_id=NULL, application_id=NULL, channel=cli and remain queryable."]
    :checker "node scripts/check-v3-conversation-session-management.mjs")

  (interaction-ledger
    :desc "Durable replay ledger for external/client-channel interaction runs. Conversation messages are the human read model; interaction-ledger is the event-level execution trace."
    :schema "missiond.interaction-ledger.v1"

    (interaction-run-correlation
      :entry [InteractionEnvelope AuthPermissionContext conversation-session-management mission_context_gather]
      :core
        [(step 1 "Resolve or create the scoped conversation using user_id, tenant_id, application_id, channel, and topic_id.")
         (step 2 "Mint one stable interaction_id for the client request and carry it through intent, plan, BoardTask, worker, artifact, follow, and replay.")
         (step 3 "Bind conversation_id, grounding_context_id, intent_artifact_id, plan_artifact_id, BoardTask ids, and result artifact hash into runtime_metadata or raw_data.")
         (step 4 "Do not let confirmation text such as 确认意图 or 确认 plan overwrite topic identity.")]
      :egress [interaction_run_metadata runtime_metadata topic_binding]
      :surfaces ["/interactions/v1/messages" "/jarvis/v1/chat/completions" mission_interaction])

    (interaction-event-ledger
      :entry [SSEEvent SharedArtifact BoardTaskEvent TaskResultArtifact TypedDiagnostic]
      :core
        [(step 1 "Persist every user-visible lifecycle event before or alongside streaming it to the channel response sink.")
         (step 2 "Store first-wave events in conversation_events with event_type prefixed by interaction. and raw_data containing interaction_id, event_kind, phase, artifact ids, task ids, and diagnostics.")
         (step 3 "Treat PTY and provider screen state as diagnostic evidence only; completion authority is task-result-artifact or terminal typed diagnostic.")
         (step 4 "Use final only for terminal task result or terminal diagnostic; dispatch_accepted and result_pending are non-terminal.")]
      :egress [conversation_events interaction_event_stream task_result_artifact]
      :surfaces [persist_interaction_event insert_conversation_events_batch])

    (interaction-replay-api
      :entry [interaction_id PermissionContext conversation_events]
      :core
        [(step 1 "GET /interactions/v1/:interaction_id/events queries the durable ledger rather than returning a static placeholder.")
         (step 2 "Replay events in insertion order with schema missiond.interaction-event-stream.v1 and exact event names stripped from interaction.*.")
         (step 3 "When DB is unavailable, return a typed replay_unavailable diagnostic without fabricating state.")
         (step 4 "MCP mission_interaction status/follow must converge on the same ledger-backed view as the HTTP replay API.")]
      :egress [SSEReplay InteractionStatus ConversationAudit]
      :surfaces ["/interactions/v1/{interaction_id}/events" get_interaction_events mission_interaction])

    (conversation-control-plane
      :entry [conversation_messages conversation_events shared_artifacts board_task_runtime_metadata task_result_artifact]
      :core
        [(step 1 "Merge user/assistant messages, interaction events, context-gather artifacts, intent/plan artifacts, BoardTask metadata, worker conversation ids, and final artifacts by conversation_id and interaction_id.")
         (step 2 "Default list/get/search to the caller's Auth-derived isolation scope and expose legacy_unscoped rows only as explicitly labeled migration evidence.")
         (step 3 "Generate compact Lisp context capsules from related conversation/topic history plus SSOT, active KB, project registry, skill evidence, and infra facts.")
         (step 4 "Expose a replayable audit timeline for mobile/web/debug without forcing the user to manually create a conversation.")]
      :egress [conversation_replay topic_context_capsule interaction_audit_view]
      :surfaces [mission_conversation_query mission_context_gather mission_interaction])

    :storage
      (:first-wave "conversation_events"
       :event-type-prefix "interaction."
       :interaction-id-path "raw_data.interaction_id"
       :future-normalized-tables [interaction_runs interaction_events])

    :events [received authenticated permission_resolved grounding intent_draft intent_archived key_judgment_draft plan_draft plan_archived confirm_required communicator_status communicator_final board_task_created worker_dispatched worker_status dispatch_accepted result_pending result_artifact diagnostic final]

    :invariants
      ["Every Jarvis/Web/iOS/WeChat interaction milestone visible to a client MUST be persisted into interaction-ledger with the same interaction_id."
       "GET /interactions/v1/{interaction_id}/events MUST replay durable conversation_events rows and MUST NOT return a static placeholder."
       "Final means terminal task-result-artifact or terminal typed diagnostic; non-terminal dispatch/result_pending events MUST NOT be projected as final."
       "Jarvis default iOS/openai flow MUST ledger intent_archived and plan_archived for replay without emitting user-blocking confirmation cards; confirm_required remains ledgered only for explicit manual-review/legacy confirmation mode so clients can resume after network loss."
       "communicator_status and communicator_final events MUST be ledgered for any AGY/Gemini communication-officer user-facing status or final result summary."
       "BoardTask runtime_metadata and interaction raw_data MUST preserve grounding_context_id, intent_artifact_id, plan_artifact_id, worker_conversation_id, and task_result_artifact_hash when available."
       "Topic labels MUST be derived from the user request/topic, not from conservative confirmation text."
       "PTY output, provider screen state, and Board notes are evidence/projection only and are never the interaction completion authority."
       "The interaction replay API and mission_interaction status/follow MUST converge on the same durable ledger."]
    :checker "node scripts/check-v3-interaction-ledger-isomorphism.mjs --json")
