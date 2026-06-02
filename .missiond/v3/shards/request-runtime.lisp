  (grounding-search-aggregate
    :schema "missiond.grounding-search-aggregate.v1"
    :purpose "Provide one high-frequency fact-gathering entry before intent.lisp, plan.lisp, Board triage, deploy decisions, or worker delegation so operators do not have to remember every retrieval surface."
    :primary-tool mission_context_gather
    :default-sources [runtime_truth project_ssot reviewed_kb active_board support_refs]
    :typed-evidence-lanes [runtime_truth project_ssot reviewed_kb active_board skill_evidence conversation_audit cold_archive support_refs]
    :source-policy
      ((source source-profile
         :tool mission_context_gather
         :profiles [intent_default deploy_ops conversation_audit full_debug]
         :default intent_default
         :rule "source_profile is resolved before retrieval: intent_default allows runtime_truth/project_ssot/reviewed_kb/active_board/support_refs; deploy_ops adds scoped skill_evidence and redacted credential refs; conversation_audit adds bounded conversation episode/fact evidence; full_debug enables raw/cold forensics.")
       (source runtime-environment
         :tool mission_context_gather
         :scope deployed-runtime-authority
         :rule "For deployed MissionD, mission_context_gather always includes runtime_environment: MISSIOND_RUNTIME_DIR, MISSIOND_COMPILED_RUNTIME_DIR, repo_runtime_dir, canonical Jarvis monitor endpoints, the rule that repo .missiond/v3/runtime/** is dev/cold evidence only, and the MISSIOND_RUNTIME_DIR/context-gather-worker ignored mirror rule for provider CLIs that cannot read outside their workspace; repo .missiond/v3/runtime/context-gather-worker/** is dev fallback only.")
       (source active-board-task-records
         :tool mission_board_query
         :scope active
         :rule "Task records are retrieval evidence and must be searchable through FTS/embedding, but broad historical/done Board backlog is excluded unless include_historical=true.")
       (source bounded-conversation-logs
         :tool mission_conversation_query
         :scope query-scoped
         :default-time-range last_30d
         :rule "Durable provider/user conversations are searched only when conversation_audit, full_debug, or include_conversations=true opts in; project/time/conversation_type filters must be enforced identically by hybrid vector and FTS paths.")
       (source skill-operational-evidence
         :tool mission_skill_context
         :rule "Skill files are operational evidence for ClaudeCode-compatible workers; mutation of skill files must be delegated through skill-edit-delegation-policy. Default intent_default does not scan skill/infra evidence unless skill, infra_target, include_infra=true, or deploy_ops opts in. deploy_ops infra evidence must still be target/project/query scoped; it must not perform global skill evidence scans. When infra-os is disabled in kernel-core mode, context_gather MUST return a feature_disabled source summary and diagnostic instead of a silent empty infra lane.")
       (source credential-refs
         :tool mission_infra_query
         :scope explicit-opt-in
         :rule "credential_refs MUST NOT be emitted unless include_credentials=true, source_profile=deploy_ops, or source_profile=full_debug.")
       (source active-kb
         :tool mission_kb_query
         :rule "Default retrieval applies knowledge_review_state overlay and excludes archived/superseded/noise memories.")
       (source ssot-intent
         :tool mission_intent
         :rule "Active SSOT Lisp is the long-lived project fact authority; cold runtime evidence is opt-in.")
       (source response-source-compaction
         :tool mission_context_gather
         :scope default-response
         :rule "mission_context_gather default responses expose source_summaries, evidence_lanes, evidence_items, support_catalog, and raw_sources_omitted=true; legacy raw sources are returned only when include_raw_sources=true or source_profile=full_debug. evidence_refs in compact mode are derived from compact summaries, not raw skill/conversation/source payloads."))
    :functions
      ((function context-gather-before-intent
         :entry [user-request BoardTaskCreated external-intent-envelope unknowns-inventory]
         :core ((step s1 :logic "ask unknowns-first: what facts are still missing before judging user intent?")
                (step s2 :logic "call mission_context_gather once with query/unknowns/project/skill/infra_target and default sources")
                (step s3 :logic "synthesize evidence_refs and remaining unknowns into intent.lisp review packet")
                (step s4 :logic "write high-confidence inferred user intent as memory:decision candidate only after evidence refs are attached"))
         :egress [context-gather-result intent-review-packet intent-memory-candidate])
       (function task-record-indexing
         :entry [BoardTask workflow_run task-result-artifact audit-event]
         :core ((step s1 :logic "index BoardTask title/description/status/project/category, workflow_run summary, task-result-artifact summary, and audit event captions into the memory provider search corpus")
                (step s2 :logic "dedupe by source_type/source_id and preserve source authority so Board noise does not become active KB memory")
                (step s3 :logic "make active task records searchable by mission_context_gather without preloading full Board backlog"))
         :egress [fts-document embedding-document retrieval-evidence-ref]))
    :invariants
      ["mission_context_gather MUST expose source_profile, source_summaries, evidence_lanes, evidence_items, support_catalog, authority_order, noise_diagnostics, raw_sources_omitted, and context_noise_metrics."
       "mission_context_gather MUST normalize legacy source calls into typed EvidenceItem lanes: runtime_truth, project_ssot, reviewed_kb, active_board, skill_evidence, conversation_audit, cold_archive, and support_refs."
       "mission_context_gather(persist_read_model=true, the default) MUST persist context_gather_runs metrics and evidence_items compact projections without creating worker artifacts, deleting raw historical material, or injecting raw sources; persist=true additionally creates the context-pack artifact/capsule and forces read-model persistence."
       "mission_context_gather source_profile=intent_default MUST exclude bounded conversation logs, global skill/infra evidence, and credential_refs unless an explicit opt-in flag or deploy/debug profile enables them."
       "mission_context_gather source_profile=deploy_ops MUST pass query/project scope into mission_infra_query skill_evidence and credential_refs; evidence-only lanes MUST reject unrelated global skill hits."
       "mission_context_gather source_profile=deploy_ops infra skill_evidence MUST recognize deployment-closure evidence anchors such as service.manifest.toml, manifest gate, db adoption, migration/relation failures, compose, entrypoint, binary markers, image markers, and volume overrides while still applying query/project scope before returning items; skill-file context fallback may admit sibling evidence only when the returned line itself carries a strong closure anchor, not generic canary/healthcheck/deploy-agent prose."
       "mission_context_gather source_profile=deploy_ops MUST distinguish no infra evidence from infra-os feature_disabled by returning source_summaries.infra.status=feature_disabled and diagnostics when MISSIOND_FEATURE_INFRA_OS_ENABLE/MISSIOND_FULL_OS_ENABLE is not enabled; optional feature_disabled diagnostics MUST NOT set the top-level ok=false by themselves."
       "mission_context_gather support_catalog MUST project compiled service runtime plus compiled-deployment-policy into deployment_closure evidence covering service.manifest.toml, ReleaseLease, RuntimeObservation, ReleaseEvidence, ClosureVerdict, canary/smoke/runtime digest/binary marker/db adoption search anchors."
       "mission_context_gather MUST aggregate runtime_environment, KB, active SSOT, project registry, skill operational evidence, infra evidence, active Board task records, and bounded conversation logs through authority-aware evidence lanes rather than one flat prompt preload."
       "Board/task/workflow records are searchable retrieval evidence, not active long-term memory unless promoted by an explicit review workflow."
       "Conversation logs are searched by query and bounded window; they are not default prompt preloads."
       "Tool responses MUST include source_summaries/evidence_lanes lane summaries by default and omit raw sources unless include_raw_sources=true or source_profile=full_debug."
       "Worker context packs MUST include evidence_lanes, evidence_items, and support_catalog lane summaries by default and omit raw sources unless include_raw_sources=true or source_profile=full_debug."
       "If mission_context_gather cannot answer a source, it returns source-specific diagnostics instead of making the resident master guess."]
    :checker "node scripts/check-v3-memory-kb-isomorphism.mjs")

  (grounded-dispatch-policy
    :schema "missiond.grounded-dispatch-policy.v1"
    :purpose "Make unknowns-first context gathering a runtime gate before broad Board/Jarvis/master tasks can reach provider PTYs; prompt hints are not sufficient."
    :authority [mission_context_gather mission_task_delegate mission_swarm_run autopilot-runtime shared-memory task-result-artifact]
    :bypass-policy
      ((case exact-shard
         :requires [exact_shard_ready accepted_shard_id context_pack_path write_scope]
         :rule "Exact implementation shards already produced by intent/plan/workflow may dispatch without re-gathering, but must reference their accepted shard and scoped write surface.")
       (case emergency-code-first
         :requires [emergency_code_first visible-backfill-boardtask]
         :rule "Emergency code-first is allowed only as an explicit exception and must create Lisp/checker/evidence backfill."))
    :artifact
      ((kind context-gather-artifact
         :schema "missiond.context-gather-artifact.v1"
         :id-field grounding_context_id
         :storage "shared_artifacts(kind=context-gather)"
         :fields [unknowns query project_id source_profile sources_used source_summaries evidence_lanes evidence_items support_catalog authority_order noise_diagnostics context_noise_metrics raw_sources_omitted raw_sources_policy evidence_refs diagnostics grounded_intent_summary runtime_environment context_pack_path context_pack_file canonical_context_pack_file evidence_lane_persistence]
         :rule "mission_context_gather(persist=true) returns grounding_context_id, shared-artifact context_pack_path, canonical_context_pack_file under MISSIOND_RUNTIME_DIR, and a bounded worker-readable context_pack_file mirror under ignored MISSIOND_RUNTIME_DIR/context-gather-worker/** when deployed; repo .missiond/v3/runtime/context-gather-worker/** is a dev fallback only. Worker prompts receive only this small context slice plus confirmed intent/plan artifact refs and accepted execution metadata, not broad KB/history preloads.")
      (kind task-result-artifact
         :schema "missiond.task-result-artifact.v1"
         :id-field artifact_hash
         :storage "shared_artifacts(kind=task-result)"
         :fields [task_id project_id provider result_status summary content details raw_evidence source]
         :rule "Jarvis, Board, and worker completion surfaces MUST write task-result-artifact before streaming result_artifact/final to clients. The canonical :content field is the cleaned terminal result only; raw PTY screens, provider transcripts, tool/progress frames, escaped UI captures, echoed worker prompt contracts, and Board summary notes remain details/raw_evidence projections. Board summary notes may reference an existing task_result_artifact hash but MUST NOT be promoted into a new canonical artifact by Jarvis follow-up supervision. Worker prompts MUST ask for structured Findings/Evidence/Recommendations/Verification output, but an echoed output-contract heading block with empty Findings/Evidence/Recommendations/Verification sections is not a valid result. Worker prompts MUST NOT instruct provider workers to mark the BoardTask done before artifact settlement. Durable provider finals for reused Codex/Claude/Agy sessions must satisfy the current BoardTask output contract from runtime_metadata.dispatch_metadata before they can close or project a result artifact; stale progress/final text from an older turn is ignored until the current task artifact lands.")
      (kind task-result-artifact-idempotency
         :schema "missiond.task-result-artifact-idempotency.v1"
         :id-field artifact_hash
         :storage "task_result_artifacts"
         :fields [task_id slot_id conversation_id provider result_status summary artifact_hash deduped]
         :rule "Provider settle loops, Jarvis follow-up supervision, and Board note revalidation may observe the same final more than once. task_result_put MUST check for an existing row with the same task_id, slot_id, conversation_id, provider, result_status, and summary, then return that artifact_hash with deduped=true instead of writing timestamp-only duplicate artifacts or emitting duplicate task_result_artifact.created events.")
      (kind interaction-direct-answer
         :schema "missiond.interaction-result-artifact.v1"
         :id-field artifact_hash
         :storage "shared_artifacts(kind=interaction-direct-answer)"
         :fields [interaction_id grounding_context_id intent_artifact_id plan_artifact_id execution_mode requires_board_task answer_policy provider content sources_used]
        :rule "After mandatory intent.lisp and plan.lisp confirmation, execution_mode=grounded_direct_answer with requires_board_task=false answers through provider-interaction-box mode=grounded-direct-answer, streams answer_delta events, and writes this interaction result artifact before terminal final. The default provider is codex_cli via MISSIOND_JARVIS_DIRECT_ANSWER_PROVIDER; selected providers must be registered provider-box drivers. Missing provider-box auth, unavailable provider CLI, or missing matched durable final is a typed failure and must not fall back to BoardTask, xjpcode text-only, local code search, or fabricated answers.")
      (kind provider-interaction-turn
         :schema "missiond.provider-interaction-turn.v1"
         :id-field turn_id
         :storage "shared_artifacts(kind=provider-interaction-turn)"
         :fields [turn_id lease_id interaction_id task_id provider engine slot_id mode prompt_hash attachment_refs correlation_id durable_source provider_conversation_id final_text artifact_hash diagnostics step_records slot_status screen_identity screen_usage model model_profile provider_box_lane xjp_request_stage dangerously_bypass_approvals_and_sandbox output_contract single_turn_policy timeout_cancel_policy usage_snapshot_ref model_catalog_ref router_export_ref model_switch_result]
	         :rule "Every MissionD-owned provider CLI call that is not a normal BoardTask worker prompt MUST pass through interactive-provider-box and persist a provider-interaction-turn record. External worker turns, runner one-shots, grounded direct answers, semantic authoring, pure-text single turns, research task turns, image-generation task turns, model-switch control turns, usage probes, model-catalog exports, slot-status observations, named control-action turns such as input/clear_input/clear_screen/exit, and guarded manual PTY observe/single-step turns all normalize through this record. Prompt turns and interactive control turns must include step_records: each PTY action records before observation, action, bounded settle delay, after observation, expected change, and verification status. The after observation MUST be taken only after a provider-defined minimum delay and either two equivalent observations or a bounded max wait, so delayed repaint/style/placeholder transitions do not advance the next action prematurely. Manual PTY handoff APIs are not semantic answer channels: screen/observe returns observation only, and pty-step accepts exactly one allowlisted key or one bounded text write without Enter, then returns before/after observation; callers must send text and Enter as separate observe-act-observe turns. Observations include slot_status plus screen_identity when visible, including Agy cli_version/account/plan/current_model/selected_model/cwd and ClaudeCode cli_version/current_model/reasoning_effort/plan/permission_mode/cwd; screen_usage is attached when an Agy /usage Model Quota page is visible, including model_quotas percent/status entries. Agy startup is a first-class provider-box state: the initial not-signed-in `Signing in...` screen is Running and may be waited for up to a short bounded grace window; if it remains active, the box records the stall, attempts Ctrl+C/Ctrl+D closure, and restarts the slot. The post-logout not-signed-in login method selector is a Blocked auth_missing surface (`agy:login_method_prompt`) whose default selected row may be `1. Google OAuth`; Enter opens a Google OAuth authorization surface (`agy:oauth_authorization_prompt`) with a browser URL and optional manual authorization-code input. Authorization codes and OAuth URL challenge/state/client_id query values are secrets in observations and step records and MUST be redacted. OAuth code text input and Enter submission are separate observe-act-observe steps, followed by a wait for ready identity, trust prompt, or explicit auth failure. Ctrl+C may be a no-op on the login selector, first Ctrl+D changes it to `press ctrl+d again to exit`, and the second Ctrl+D exits the direct AGY PTY. The first-run workspace trust screen is a Blocked startup_trust surface; the box MUST observe which option is selected, move selection only when needed, and press Enter only after `Yes, I trust this folder` is selected. Codex first-run directory trust is also a Blocked startup_trust surface; the box MUST press Enter only when `1. Yes, continue` is selected. Codex `/mcp` is a printed MCP Tools inventory with screen_mcp server rows and startup-incomplete diagnostics, not an action menu; Codex MCP reconnect MUST return a restart-required hint rather than implicitly killing the PTY. Codex research/image-generation task sources are explicit provider-box guarded lanes: they run Codex exec JSON in isolated workspaces, add only the short task prefix approved by the API, require shell/file/MCP disabled and read-only sandbox, allow only the per-lane tool allowlist, persist stdout events.jsonl as durable source, and fail closed if JSONL contains any disallowed tool/function/approval event. Agy prompt turns treat bottom/recent Braille spinner status lines such as Generating.../Working.../Loading... plus esc to cancel as active; `>` alone is not idle or completion until the active indicator disappears and the durable final matches the turn. Agy slash-command surfaces are explicit control states: `/` opens a command menu, `/c` filters it, arrow keys move selection, Enter may only complete `/clear` into the composer, and a second Enter is required before clear execution is verified by the home identity screen or by a clean composer_idle screen after selected/pending /clear command staging was observed. Agy command input and Enter MUST be separate observe-act-observe steps; exit control SHOULD type `/exit`, observe, then press Enter as a separate key action and verify the shell prompt plus Resume instruction. Ctrl+D two-step (`press ctrl+d again to exit`, then second Ctrl+D) is retained only as a fallback when `/exit` fails or the slot is already on the exit-confirmation surface. Direct AGY provider slots do not expose an enclosing shell after exit; re-entering AGY is a provider-box spawn/restart action unless the operator intentionally started a diagnostic shell PTY. Agy pure-text hidden replica slots run in provider-box isolated runtime workspaces with AGY sandbox requested when available; correlation_id is sidecar metadata and MUST NOT be injected into user prompt text. Router-facing Agy pure-text turns may carry provider_box_lane/xjp_request_stage, normalized to a short kebab-case lane; provider-box MUST allocate hidden private replicas per logical model plus lane so planning, curation, synthesis, repair, and default traffic cannot share PTY context or cleanup state. If a private text-only slot returns a matched durable final but cleanup cannot be verified, the box MUST recycle that private slot before it can serve another router turn and emit a diagnostic rather than silently reusing polluted context. The box captures an Agy transcript cursor before prompt paste and extracts the assistant final only from post-cursor transcript_full.jsonl/history.jsonl steps that match the submitted prompt; any tool call, file/shell/web/MCP/approval/subagent step in that slice is a pure-text violation. The box sends input through an interactive PTY only when the mode is a prompt/control turn, correlates prompt turns against durable provider logs, emits diagnostics when no durable final exists, and may project interaction-direct-answer or task-result-artifact only after a matched durable final or canonical artifact is available. semantic-terminal remains a screen recognition source, not the owner of control APIs or durable semantic output.")
      (kind provider-usage-snapshot
         :schema "missiond.provider-usage-snapshot.v1"
         :id-field snapshot_id
         :storage "shared_artifacts(kind=provider-usage-snapshot)"
         :fields [snapshot_id provider engine slot_id account_ref model observed_at status remaining limit reset_at source confidence block_kind model_quotas diagnostics]
         :rule "Usage information is observational and confidence-scored. exact requires structured provider data, durable log metadata, a recognized usage meter, or a provider account/status surface. Agy usage-probe reads screen_usage from the /usage Model Quota page after the refresh sequence Esc -> /usage -> Enter when needed. blocked captures auth_missing, billing_or_account, usage_limit, or rate_limit. unknown is required when the provider exposes only vague text or no stable surface; MissionD must not invent remaining counters.")
      (kind provider-model-catalog
         :schema "missiond.provider-model-catalog.v1"
         :id-field catalog_id
         :storage "shared_artifacts(kind=provider-model-catalog)"
         :fields [catalog_id provider engine account_ref discovered_at source entries diagnostics]
         :rule "Model catalogs are discovered by provider-specific box drivers from stable catalog sources, recognized model pickers, provider config, or account metadata. Each entry must include provider_model_id, display_name, family, routeable_default, switch_capability, usage_probe_capability, and confidence. Agy transcript messages with model=nil/usage=nil are not catalog evidence.")
      (kind provider-router-export
         :schema "missiond.provider-router-export.v1"
         :id-field export_id
         :storage "shared_artifacts(kind=provider-router-export)"
         :fields [export_id catalog_id provider engine router_backend_ids routeable_entries blocked_entries policy_ref diagnostics]
         :rule "Router export consumes provider-model-catalog plus usage/model-switch evidence from the box. An entry becomes routeable only when it has a stable provider_model_id, a supported switch or spawn policy, a current usage status that is not blocked, and router policy approval. Router code must not scrape provider CLIs directly. Agy router export emits xjp-router MissionDAgy provider routes modeled after the Meow61 provider-source pattern, with text=true/tools=false/vision=false and provider_model_id set to the MissionD provider-box base URL from the self-built proxy deployment program. The models API also exposes provider_text_only_sources[] for xjpcode and other internal callers; each source names the exportable AGY display model, logical slot_pool_id, provider-box-managed hidden replica policy, text-only request template, and guard capabilities, never the private replica slot ids. Router callers may pass provider_box_lane/xjp_request_stage, but the public source remains the logical model; MissionD owns lane-to-hidden-replica mapping and does not export lane replica ids as router models. Agy text-only sources MUST declare prompt_instruction=false, sidecar_correlation=true, transcript_cursor_guard=true, isolated_runtime_workspace=true, durable_jsonl_guard=true, and tools/files/shell/MCP/vision=false. Provider-box also exposes guarded Codex exec text sources for GPT-5.5 xhigh/default only as codex_exec_text entries: they run `codex exec --json --output-last-message` in an isolated runtime workspace with --ignore-user-config, --ignore-rules, shell_tool/web_search/apps/view_image disabled, read-only sandbox, approval_policy=\"never\" config, and JSONL tool/function/MCP event detection; output-last-message alone is never treated as proof that no tools ran, and they are routeable only after live smoke proves no tool events for marker prompts. Provider-box additionally exposes provider_task_sources[] for guarded Codex research and image-generation task lanes: research prepends `帮我在互联网上进行详细调研以下问题：` and allows only web_search; image-generation prepends `帮我生成一张图片，要求如下：`, keeps Codex user config/skills and first-party image-generation tool surface available so the built-in imagegen skill can run, disables MCP/shell/web/view-image tools, allows only the observed node_repl bootstrap/continuation that reads or slices `$CODEX_HOME/skills/.system/imagegen/SKILL.md`, runs in a read-only isolated workspace, and treats Codex rollout JSONL `image_generation_end.saved_path` plus PNG validation as pre-import evidence, then requires authenticated xjp-image-service import and signed URL/media_artifact return as the durable result rather than trusting `output-last-message` or returning local paths. GPT-5.5 default text declares max_concurrent=4, GPT-5.5 xhigh text declares max_concurrent=2, and Codex research plus Codex image-generation each declare max_concurrent=1 on their provider-box-owned logical queue keys; the driver must acquire that per-source queue before spawning codex exec and release it only after process exit, JSONL validation, and result extraction have completed. Research remains routeable after web-search smoke; image-generation is routeable only after live smoke proves a generated PNG can be imported into xjp-image-service and returned as a signed image artifact. Provider-box may apply an AGY-wide text turn queue above per-slot queues when the AGY shared backend is unstable under concurrent active turns; router callers still see logical models, not private slots. GPT-OSS is not exported as a provider-box text-only source. ClaudeCode slot-scoped spawn/status/restart is a provider-box control flow; when a caller explicitly requests `dangerously_skip_permissions`, the box starts the PTY through `claude --dangerously-skip-permissions` using an internal capability override rather than exposing a direct subprocess path. ClaudeCode slot-scoped model switching is a verified provider-box control flow for taught IDs only: it types `/model claude-opus-4-6` or `/model claude-sonnet-4-6` as text, presses Enter as a separate step, and verifies screen_identity.current_model becomes Opus 4.6 or Sonnet 4.6 before returning success. ClaudeCode slot-scoped permissions switching is a verified provider-box control flow for taught modes only: it observes screen_identity.permission_mode, computes Shift+Tab steps in the auto/default/accept_edits/plan cycle, verifies each footer transition, and refuses bypass_permissions as a hot switch because that mode belongs to spawn/restart. MissionD serves the internal adapter as GET /provider-box/v1/models, GET /provider-box/v1/usage, POST /provider-box/v1/usage/refresh, POST /provider-box/v1/turns, POST /provider-box/v1/text-only/completions, POST /provider-box/v1/research/completions, POST /provider-box/v1/image-generation/completions, plus slot-scoped spawn/status/session/capabilities/screen/observe/input/pty-step/switch-model/permissions/usage/mcp/status/mcp/reconnect/clear-input/exit/restart/clear/completions APIs behind bearer-token auth. Codex /exit is a verified control action and explicit restart requires confirm_destroy_context=true; Codex MCP reconnect is advertised as restart_required rather than hot-reconnect capable.")
      (kind provider-text-only-source
         :schema "missiond.provider-text-only-source.v1"
         :id-field source_call_id
         :storage "shared_artifacts(kind=provider-text-only-source)"
         :fields [source_call_id provider_id engine_id model input_text output_text no_tools no_mcp no_shell no_file_access proposal_kind proposal_artifact_hash]
         :rule "Legacy ClaudeCode, Codex CLI, Agy, or any paid CLI used as a proposal source for xjpcode/MissionD planning MUST run in no-tools text-only mode while migration is incomplete. They may propose intent, plan, decomposition, risks, or review text, but MUST NOT execute shell, MCP tools, file reads, file writes, hidden subagents, or PTY tool loops inside this source role. New proposal sources MUST use provider-interaction-turn through interactive-provider-box; Codex exec is allowed only through the codex_exec_text provider-box guarded lane with tool-disabling config plus JSONL violation detection, not as an ad-hoc caller subprocess.")
      (kind plan-atomization-graph
         :schema "missiond.plan-atomization-graph.v1"
         :id-field atom_graph_id
         :storage "shared_artifacts(kind=plan-atomization-graph)"
         :fields [atom_graph_id interaction_id grounding_context_id intent_artifact_id plan_artifact_id shard_nodes atom_tasks dependency_edges serial_groups parallel_groups predicted_tool_sequence context_sources detour_budget]
         :rule "A confirmed plan.lisp is a prediction and decomposition input, not a worker prompt. Before implementation dispatch, MissionD compiles it into shard_nodes and atom_tasks. Every atom task MUST declare execution_order=serial or execution_order=parallel, serial atoms MUST be represented by dependsOn/dependency_edges, parallel atoms MUST share a parallel_group without mutual dependencies, and provider workers receive only atom-level context slices."))
    :functions
      ((function context-gather-artifact
         :entry [mission_context_gather unknowns-inventory BoardTask source_id project_id]
         :core ((step s1 :logic "derive query from explicit unknowns or the raw objective; never use broad historical preload as the query source")
                (step s2 :logic "query runtime_environment, project registry, active SSOT, active KB, skill evidence, infra/deploy facts, active Board task records, bounded conversations, and tool directory through the aggregate")
                (step s3 :logic "return source-specific diagnostics for missing or stale authorities instead of letting the worker guess")
                (step s4 :logic "persist the payload into shared_artifacts(kind=context-gather), materialize the canonical context pack under MISSIOND_RUNTIME_DIR/context-gather when deployed, materialize a worker-readable ignored mirror under .missiond/v3/runtime/context-gather-worker for workspace-confined provider CLIs, and return grounding_context_id plus shared-artifact context_pack_path")
                (step s5 :logic "Jarvis worker prompts must prefer context_pack_file; if unavailable, they may use mission_shared_memory(action=artifact_get, hash=...) or mission_context_slice. Opaque artifact URIs without retrieval instructions are invalid")
                (step s6 :logic "Jarvis worker prompts must include target engine/pool, write_policy, read/write scope, confirmed intent_artifact_id, confirmed plan_artifact_id, and a compact accepted execution slice for no-MCP workers"))
         :egress [grounding_context_id context_pack_path context_pack_file canonical_context_pack_file sources_used diagnostics shared_artifact])
       (function plan-atomization-compiler
         :entry [confirmed-plan.lisp grounding_context_id intent_artifact_id plan_artifact_id provider-text-only-source]
         :core ((step s1 :logic "treat plan.lisp as a high-level forecast of the route, risks, evidence, and expected implementation surfaces; never dispatch it directly to a worker")
                (step s2 :logic "optionally ask ClaudeCode/Codex/Agy proposal sources through provider-interaction-box mode=pure-text-single-turn with no tools, no shell, no file reads, no MCP, and no hidden subagents; legacy provider-text-only-source remains migration-only")
                (step s3 :logic "compile the accepted plan into shard_nodes, then recursively split each shard into atom_tasks whose objective is small enough for a low-skill worker to execute or verify")
                (step s4 :logic "attach context_sources, predicted_tool_sequence, acceptance, read_scope, write_scope, and detour_budget to each atom")
                (step s5 :logic "derive dependency_edges, serial_groups, and parallel_groups; serial atoms lower to BoardTask dependsOn, while parallel atoms lower to independent BoardTasks sharing a parallel_group")
                (step s6 :logic "persist plan-atomization-graph and write atom_task_id, atom_path, execution_order, dependency_policy, and parallel_group into BoardTask runtime_metadata/task_contracts"))
         :egress [plan-atomization-graph atom_task_contracts BoardTask.runtime_metadata task_contracts])
       (function xjpcode-atom-worker-runtime
         :entry [atom_task_contract context_capsule_lisp read_scope write_scope tool_policy xjpcode-worker]
         :core ((step s1 :logic "xjpcode receives atom-level work-order context and may decide which scoped repo-local facts to gather")
                (step s2 :logic "read-only mode may use list_files/read_file/ripgrep/git_status/git_diff/run_check within read_scope; write mode additionally requires accepted_shard_id, write_scope, and MissionD write lease")
                (step s3 :logic "every tool call emits an event with atom_task_id, tool_id, scope, duration, and summarized output; large output spills through artifact storage")
                (step s4 :logic "on missing context, xjpcode emits fact_request/detour telemetry instead of silently widening scope")
                (step s5 :logic "completion writes task-result-artifact bound to atom_task_id and parent BoardTask"))
         :egress [agent-runtime-events task-result-artifact worker-telemetry fact_request])
       (function worker-detour-telemetry
         :entry [provider-tool-event xjpcode-tool-event worker-final atom_task_contract]
         :core ((step s1 :logic "compare actual tool sequence, files read, files changed, and extra searches against predicted_tool_sequence and context_sources")
                (step s2 :logic "classify detours as decomposition-gap, context-gap, scope-gap, tool-gap, or worker-improvisation")
                (step s3 :logic "write telemetry to worker-capability-telemetry and attach improvement candidates to the parent plan-atomization-graph")
                (step s4 :logic "when detours recur, create a workflow/checker/SSOT optimization task rather than blaming the worker prompt"))
         :egress [worker-capability-telemetry decomposition-gap-report workflow-improvement-candidate])
       (function provider-interaction-box-turn
         :entry [JarvisSSE runner worker external-app provider-interaction-request]
         :core ((step s1 :logic "normalize every non-BoardTask provider CLI request into missiond.provider-interaction-turn.v1 with provider, mode, prompt_hash, attachment_refs, timeout, cwd/project_root, and correlation_id")
                (step s2 :logic "validate caller capability, read/write scope, desired_worker lease, no_tools/no_mcp/no_shell/no_file_access guard, and requested model/profile against workstation-pool and router policy")
                (step s3 :logic "select or spawn an interactive PTY slot from workstation-pool; direct `claude --print`, ad-hoc `codex exec`, Agy print/prompt modes, Gemini `-p -o stream-json`, and stdin-closed provider subprocesses are forbidden outside the legacy migration inventory and explicit provider-box guarded lanes such as codex_exec_text, codex_research, and codex_image_generation")
                (step s4 :logic "dispatch by mode: prompt turns call driver submit-turn, slot-status calls driver status/observe, control-action calls named driver input/clear_input/clear/exit operations, model-switch calls driver switch-model or respawn, usage-probe calls driver usage-probe, model-catalog-export calls driver model-catalog and router exporter, pure-text-single-turn additionally installs the pure-text guard")
                (step s5 :logic "for every provider UI action, run observe -> act -> observe -> verify -> record; do not send the next key/input until the previous step has a verified or explicitly handled ambiguous/unchanged/failed status")
                (step s6 :logic "for prompt turns, submit input as human-like terminal input and use provider-specific interactive attach UI when attachments are present, or return PROVIDER_INTERACTIVE_ATTACHMENT_UNSUPPORTED")
                (step s7 :logic "wait for provider idle/complete state after provider-specific active indicators disappear; for Agy, `>` alone is not completion while a bottom/recent Braille spinner with Generating.../Working.../Loading... or esc to cancel is still visible. If timeout_cancel_policy detects running_timeout_secs or no_progress_grace_secs exhaustion, record PROVIDER_TURN_STALLED, send the configured cancel key such as escape, verify Interrupted · What should Antigravity CLI do instead? plus ? for shortcuts/current model or another provider-ready surface, then retry only within max_retries. Then read durable provider logs such as ClaudeCode JSONL, Codex rollout JSONL, Agy transcript_full.jsonl/history.jsonl, or Gemini session files for the matching turn evidence; Agy text-only hidden replicas use a pre-submit transcript cursor plus submitted prompt match rather than prompt-injected correlation text")
                (step s8 :logic "write provider-interaction-turn plus interaction-direct-answer, task-result-artifact, provider-usage-snapshot, provider-model-catalog, or provider-router-export projection only after matched evidence exists")
                (step s9 :logic "on timeout, PROVIDER_TURN_TIMEOUT_CANCEL_FAILED, provider auth/billing/quota block, unsupported driver capability, missing durable final, stale final mismatch, model-switch unverified, usage unknown, step verification failure, or pure-text guard violation, emit typed diagnostics without synthesizing an answer from PTY screen text"))
         :egress [provider-interaction-turn interaction-direct-answer task-result-artifact provider-usage-snapshot provider-model-catalog provider-router-export diagnostic])
       (function provider-box-model-control
         :entry [external-app runner worker provider-interaction-turn model-switch-request]
         :core ((step s1 :logic "load the active lease/slot and provider driver capability for the target provider/model")
                (step s2 :logic "if the provider supports launch_arg only, stop/recreate the PTY under the same lease with the requested model/profile and mark previous provider_conversation_id closed for routing")
                (step s3 :logic "if the provider supports interactive_ui or conversation_setting, invoke the driver and verify the new model through durable metadata, recognized UI/footer, provider settings export, or a fresh launch record")
                (step s4 :logic "if verification is missing, return MODEL_SWITCH_UNVERIFIED and do not update router routeability or slot_current_model projections")
                (step s5 :logic "if unsupported, return MODEL_SWITCH_UNSUPPORTED with provider/model/capability diagnostics"))
         :egress [provider-interaction-turn model_switch_result diagnostic])
       (function provider-box-slot-control
         :entry [external-app runner worker provider-interaction-turn slot-status-request control-action-request]
         :core ((step s1 :logic "normalize spawn/status/screen/observe/input/pty-step/clear-input/clear/exit requests into provider-interaction-turn rather than direct PTY writes by callers")
                (step s2 :logic "status/screen/observe observes the slot and returns slot_status plus screen_identity/screen_usage and the current PTY observation when visible; it does not spawn unless spawn_if_missing=true and does not create semantic answer text")
                (step s3 :logic "input writes to the provider composer with optional Enter and records observe-act-observe verification; it does not wait for durable final extraction")
                (step s3b :logic "pty-step is the guarded manual handoff path for exceptional external LLM recovery: it accepts exactly one allowlisted key or one bounded text write without Enter, records before/after observation, returns typed verification, and requires text and Enter as separate API calls")
                (step s4 :logic "clear_screen and exit execute provider-specific learned UI flows, verify the recognized post-action state, and return typed unverified/failed diagnostics when confirmation is missing")
                (step s5 :logic "semantic-terminal remains a parser/recognizer dependency; MissionD provider-box owns API contracts, lease selection, queueing, retries, and durable-output policy"))
         :egress [provider-interaction-turn slot_status pty_observation diagnostic])
       (function provider-box-usage-and-catalog-export
         :entry [router provider-interaction-request usage-probe model-catalog-export]
         :core ((step s1 :logic "read cached provider usage snapshots through GET /provider-box/v1/usage without touching provider PTYs; engine/provider query selects AGY or Codex cache; refresh only through POST /provider-box/v1/usage/refresh")
                (step s1b :logic "ask the provider driver for a usage snapshot; classify exact, estimated, blocked, or unknown with source and confidence. Codex usage refresh runs /status on a dedicated probe PTY and returns only the current `5h limit` and `Weekly limit` lines, ignoring later secondary-model quota sections.")
                (step s2 :logic "ask the provider driver for a model catalog only from stable catalog/config/model-picker/account sources; for Agy, open /model interactively, scroll the picker, and collect display names from recognized screen_identity/model-picker observations")
                (step s3 :logic "write provider-model-catalog even when some entries are blocked or unsupported, but keep routeable=false until switch/test/usage evidence exists")
                (step s4 :logic "publish provider-router-export only through router policy, never by letting router scrape a provider CLI")
                (step s5 :logic "for Agy, emit MissionDAgy TOML route entries only for observed exportable model-picker entries with a provider-box-managed logical slot_pool_id, hidden replica policy, pure_text=true, usage status not blocked, and provider_model_id equal to the managed proxy URL for MissionD provider-box; do not export GPT-OSS, do not infer all models from transcript JSONL, and do not export private replica slot ids"))
         :egress [provider-usage-snapshot provider-model-catalog provider-router-export diagnostic])
       (function task-delegate-grounding-gate
         :entry [mission_task_delegate mission_swarm_run mission_plan_execute]
         :core ((step s1 :logic "classify dispatch as exact shard, emergency code-first, or broad objective")
                (step s2 :logic "for broad objective without grounding_context_id, synchronously call mission_context_gather(persist=true)")
                (step s3 :logic "fail fast with GROUNDING_REQUIRED if gather returns diagnostics or no grounding_context_id")
                (step s4 :logic "write grounding_context_id, context_pack_path, context_pack_file, sources_used, and evidence count into BoardTask metadata and prompt slice")
                (step s5 :logic "implementation swarm lanes still require accepted_shard_id and write_scope; gathered broad context may only create investigation/synthesis tasks"))
         :egress [grounded-BoardTask delegation-metadata context-pack-slice GROUNDING_REQUIRED])
       (function autopilot-grounding-gate
         :entry [BoardTaskBeforeDispatch auto_execute task-metadata]
         :core ((step s1 :logic "allow exact shard only when accepted_shard_id, context_pack_path, and write_scope are present")
                (step s2 :logic "allow broad task only when grounding_context_id is present")
                (step s3 :logic "block ungrounded broad task before PTY input and append a diagnostic Board note")
                (step s4 :logic "never re-enable hidden context prefetch as a substitute for grounded dispatch"))
         :egress [BoardTaskBlocked diagnostic-note no-PTY-dispatch])
      (function jarvis-result-artifact-gate
         :entry [JarvisSSE BoardTaskSummaryNote provider-final task-result-artifact]
         :core ((step s1 :logic "inspect worker/Board summary notes for existing task-result-artifact hash")
                (step s2 :logic "when only a Board summary projection exists, emit TASK_RESULT_ARTIFACT_REQUIRED or result_pending; Jarvis follow-up supervision never writes a canonical artifact from that summary")
                (step s3 :logic "bound artifact write latency with an explicit timeout and emit TASK_RESULT_ARTIFACT_WRITE_TIMEOUT when the writer stalls")
                (step s4 :logic "emit TASK_RESULT_ARTIFACT_WRITE_FAILED diagnostic instead of pretending a missing artifact exists")
                (step s5 :logic "if a BoardTask is done with a summary but no artifact hash, emit TASK_RESULT_ARTIFACT_REQUIRED and fail fast instead of streaming the Board note as final text")
                (step s6 :logic "stream final text only after the task-result artifact hash is known or the diagnostic is surfaced")
                (step s7 :logic "if the worker provider returns an empty final after its slot is idle/exited/error, record provider-empty-final as a task-result-candidate observation only, emit a typed diagnostic to Jarvis/follow streams, and keep terminal BoardTask state blocked on canonical task_result_put + worker_settle artifact_hash")
                (step s8 :logic "when Autopilot/watchdog observes a durable provider final for an idle running worker, it may record candidate evidence but must not synthesize a canonical task-result-artifact or change BoardTask to done; only the completion authority may close after validating an existing canonical artifact hash")
                (step s9 :logic "durable final selection is output-contract aware: for tasks declaring Findings/Evidence/Recommendations/Verification, provider messages missing those sections are treated as stale/progress evidence, not final results")
                (step s10 :logic "Autopilot PTY extraction must focus the last structured final block such as Findings/Evidence/Recommendations/Verification, Summary, or Final Report, then strip tool/progress/status lines before writing canonical artifact content")
                (step s11 :logic "Codex and similar TUIs may render final report headings as bullet-prefixed markdown such as `• ## Findings`; extractor and output-contract checks must normalize those bullets as UI framing, not as missing report headings")
                (step s12 :logic "if daemon restart or send loss leaves an idle worker slot without canonical artifact after watchdog_grace_secs, the watchdog must wait one final settle window and re-read the live PTY screen, then record an idle-slot-without-canonical-artifact observation; no terminal state is written until canonical task_result_put + worker_settle artifact_hash exists")
                (step s13 :logic "scripts/audit-stale-boardtask-finals.mjs provides a dry-run stale-final audit that flags conversation final before BoardTask claim, terminal tasks without task-result-artifact hash, and summary projections reused as final authority"))
         :egress [task-result-artifact result_artifact_event final_event diagnostic])
       (function jarvis-dispatch-causality
         :entry [JarvisSSE plan-confirmed BoardTask auto_execute]
         :core ((step s1 :logic "after plan confirmation creates an auto_execute BoardTask, emit board_task_created with grounding/intent/plan artifact ids")
               (step s2 :logic "immediately emit worker_dispatched with dispatch_state=pending_autopilot_claim, task_id, follow_payload, and terminal_task_result=false so mobile/Web clients can render the causal handoff before the asynchronous slot claim occurs")
               (step s3 :logic "later follow-up supervision may emit another worker_dispatched with the concrete slot_id once Autopilot claims a slot")
                (step s4 :logic "do not stream task completion on the initial dispatch response; emit dispatch_accepted plus result_pending/follow_payload and require follow-up artifact validation")
                (step s5 :logic "final is reserved for terminal task-result-artifact or terminal typed diagnostics; non-terminal asynchronous handoff MUST NOT emit final"))
         :egress [board_task_created worker_dispatched dispatch_accepted result_pending follow_payload])
       (function jarvis-result-followup
         :entry [JarvisSSE missiond_follow_task_id BoardTask task-result-artifact]
         :core ((step s1 :logic "public Jarvis follow routes use MISSIOND_JARVIS_PUBLIC_STREAM_BUDGET_SECS as a short-poll budget, below edge/tunnel request timeouts, so mobile/proxy clients never wait on a single long-held connection")
                (step s2 :logic "when a worker task is still running after the short public stream budget, emit result_pending with follow_payload.missiond_follow_task_id and finish the SSE cleanly")
                (step s3 :logic "a follow-up request carrying missiond_follow_task_id bypasses intent/plan regeneration and resumes observation of the existing BoardTask")
                (step s3b :logic "OpenAI-compatible /jarvis/v1/chat/completions adapters MUST detect missiond_follow_task_id before default-slot readiness checks; a busy worker slot during follow is a progress state, not JARVIS_SLOT_BUSY")
                (step s4 :logic "if the followed task is already terminal, immediately revalidate task-result-artifact and stream result_artifact/final")
                (step s5 :logic "result_pending is not a fallback answer; it is a resumable transport state and terminal_task_result remains false")
                (step s6 :logic "while supervising a still-running worker on a public/mobile follow stream, emit client-visible worker_status heartbeat events bounded by MISSIOND_JARVIS_VISIBLE_HEARTBEAT_SECS; colon SSE comments remain transport keepalive only and are not sufficient UI progress")
                (step s7 :logic "timeout, poll_timeout, or public stream budget exhaustion emits diagnostic plus result_pending, never non-terminal final")
                (step s8 :logic "when an idle-slot watchdog has no durable provider final, it must read the fresh PTY screen before using cached pty.send responses, because cached responses can be stale progress frames"))
         :egress [result_pending follow_payload result_followup_stream result_artifact_event final_event])
       (function jarvis-grounded-direct-answer
         :entry [JarvisSSE confirmed-plan grounding_context_id intent_artifact_id plan_artifact_id provider-interaction-box]
         :core ((step s1 :logic "read execution_mode and requires_board_task from confirmed plan metadata")
                (step s2 :logic "allow only execution_mode=grounded_direct_answer with requires_board_task=false")
                (step s3 :logic "load bounded grounding context preview and source refs from context_pack_file/shared artifact")
                (step s4 :logic "call provider-interaction-box mode=grounded-direct-answer with no tool schema, no write scope, and no BoardTask authority; read the answer from the matched durable provider final")
                (step s5 :logic "stream answer_delta chunks and provider diagnostics to the client")
                (step s6 :logic "write interaction-direct-answer artifact with schema missiond.interaction-result-artifact.v1 before terminal final")
                (step s7 :logic "if provider-interaction-box auth/driver/CLI readiness fails, or the provider cannot produce a matched durable final, fail fast with typed diagnostic and no fallback BoardTask"))
         :egress [answer_delta result_artifact_event final_event diagnostic]))
    :invariants
      ["All non-exact worker dispatch must carry grounding_context_id before a provider PTY receives the prompt."
       "mission_context_gather is the only default aggregate for runtime_environment/KB/SSOT/project/skill/infra/Board/conversation/tool facts; callers should not hand-roll partial context lookup."
       "Workers reviewing deployed MissionD runtime state MUST use runtime_environment.monitor_endpoints.canonical_local_http or canonical_public_https before inspecting repo .missiond/v3/runtime/**; repo runtime files are dev/cold evidence only except bounded context-gather-worker mirrors passed explicitly as worker-readable context slices."
       "Workers MUST NOT guess Jarvis monitor ports or use unix-socket probes unless a dedicated diagnostic explicitly asks for low-level socket testing."
       "Grounding artifacts are durable evidence and task metadata; hidden prompt preloads are not grounding."
       "Autopilot must block broad ungrounded BoardTasks instead of sending them to workers for self-discovery."
       "Direct local code search is allowed only after the grounding artifact identifies code surface evidence as a required source."
       "Jarvis dispatch metadata MUST derive read_scope from the active runtime/project root (MISSIOND_PROJECT_ROOT, MISSIOND_REPO_ROOT, MISSIOND_WORKSPACE_ROOT, or current daemon cwd) and MUST NOT hardcode a developer-machine root path."
       "Jarvis dispatch classification MUST let explicit read-only/no-edit constraints win over incidental implementation words such as `提交` inside `不要提交`; a task that says `只读` or `不要修改文件` MUST route as review/read-only with empty write_scope."
       "Jarvis dispatch classification MUST route broad investigation/design/planning objectives as review/read-only unless they carry an explicit exact shard or code-now marker; words like `补齐` inside `调查/设计/实施方案` are follow-up intent, not permission to skip synthesis and open a write-capable worker."
       "Jarvis worker prompts MUST explicitly forbid ClaudeCode internal Task/Explore/TaskCreate/TaskUpdate/TaskList/TaskOutput subagent tools when worker_may_delegate=false; recursive decomposition belongs to MissionD workflow, not provider-local hidden workers."
       "Jarvis result streaming MUST use task-result-artifact as canonical completion authority; Board summary notes are only projections carrying an existing artifact hash and MUST NOT be converted to artifacts by follow-up streams."
       "Idle-slot watchdog PTY extraction MUST prefer the current live screen over slot_last_responses; slot_last_responses are fallback only because they may hold stale progress captured before the provider printed its final answer."
       "Idle-slot watchdog MUST recognize Codex TUI bullet-prefixed markdown report headings (`• ## Findings`, `• ## Evidence`, `• ## Recommendations`, `• ## Verification`) as structured final artifacts."
       "Idle-slot watchdog MUST perform one final settle/re-extract pass before failing an idle task without durable final, because provider TUI final rendering and conversation ingestion can lag the first idle observation."
       "task-result-artifact content MUST be clean terminal worker output; raw PTY screen captures, provider transcript envelopes, and escaped UI/progress text are diagnostics/evidence only and cannot become canonical content."
       "Jarvis plan-confirmed dispatch MUST emit a worker_dispatched handoff event with dispatch_state=pending_autopilot_claim before returning result_pending, even if the concrete slot is claimed asynchronously later."
       "Worker prompts MUST NOT instruct provider workers to call mission_board_update(status=done) as the primary close path; workers return structured final output, and Autopilot/orchestrator closes only after task-result-artifact validation."
       "Autopilot/shared-memory result artifact writes MUST be bounded; missing or stalled artifact writes produce typed diagnostics and MUST NOT silently fall back to Board note final text."
       "Jarvis BoardTask and notes polling during public SSE supervision MUST be bounded by MISSIOND_JARVIS_DB_POLL_TIMEOUT_SECS; DB/EventBus stalls produce typed diagnostics or result_pending, never silent mobile/proxy hangs."
       "Jarvis public SSE streams MUST return a typed result_pending/follow_payload before mobile or reverse-proxy timeouts; follow-up requests with missiond_follow_task_id resume the existing BoardTask instead of creating a new intent or plan."
       "Jarvis Web/iOS clients and smoke tests MUST automatically follow result_pending.follow_payload.missiond_follow_task_id until result_artifact plus terminal final or a terminal typed diagnostic; dispatch_accepted alone is only a handoff receipt."
       "Jarvis public follow streams MUST emit visible worker_status heartbeat events during long-running worker supervision; transport-only SSE comments do not satisfy mobile UI observability."
       "Jarvis plan-confirmed dispatch MUST NOT wait for worker terminal state on the initial mobile/public SSE request; it creates the BoardTask, returns result_pending with follow_payload immediately, and only follow requests supervise task-result-artifact completion."
       "Jarvis MUST NOT emit final for dispatch_accepted, result_pending, timeout, poll_timeout, or public stream budget exhaustion; final is terminal-only and requires task-result-artifact validation or a terminal typed diagnostic."
       "Agy and other provider artifact completion MUST accept numbered markdown report headings such as `## 1. Findings`, `## 2. Evidence`, `## 3. Recommendations`, and `## 4. Verification`; provider-generated numbering is formatting, not a missing output-contract section."
       "Jarvis intent/plan confirmations MUST accept both top-level missiond_intent_confirmed/missiond_plan_confirmed fields and wrapped missiond_confirm payloads, so iOS and external clients do not need to mirror MissionD's internal JSON shape."
       "Jarvis intent/plan confirmation payloads MUST carry missiond_objective from the original request; confirmed dispatch must derive BoardTask title, worker prompt, and dispatch metadata from that objective, never from a later confirmation utterance such as `确认 plan`."
       "Plan.lisp MUST NOT be dispatched directly as a worker prompt; confirmed plans first compile to plan-atomization-graph, then to atom-level BoardTasks."
       "Each worker BoardTask created from a plan atom MUST carry atom_task_id, atom_path, execution_order, dependency_policy, and either dependsOn serial edges or a parallel_group in runtime_metadata/task_contracts."
       "Board parallel execution is explicit: execution_order=parallel tasks have no mutual dependsOn edge and share a parallel_group; execution_order=serial tasks are gated by dependsOn or by the atom graph root order."
       "Provider CLI access outside normal BoardTask worker dispatch MUST route through interactive-provider-box and persist missiond.provider-interaction-turn.v1; direct `claude --print`, ad-hoc `codex exec`, Agy print/prompt modes, Gemini `-p -o stream-json`, and stdin-closed provider subprocesses are allowed only as explicitly inventoried legacy migration targets, except provider-box codex_exec_text/codex_research/codex_image_generation guarded automation lanes that run isolated Codex exec JSON with explicit tool-deny or tool-allowlist checks and fail closed on disallowed JSONL tool/function evidence."
       "Provider CLI spawn/status/input/clear-input/clear/exit/model-switch/usage/completion APIs belong to MissionD provider-box. semantic-terminal only parses visible terminal state for those drivers and MUST NOT become the API or orchestration boundary."
       "ClaudeCode, Codex CLI, and Agy used as planning/decomposition proposal sources are legacy text-only no-tools sources only until migrated to provider-interaction-turn; xjpcode remains the governed worker runtime and may gather scoped context through its own atom tool policy."
       "Worker detours are first-class telemetry. If a worker had to discover missing context, invent a subplan, or widen search beyond predicted_tool_sequence, MissionD records a decomposition/context infrastructure gap for workflow improvement."]
    :checks ["node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"])

  (unified-entry
    :desc "request -> intent alignment -> plan -> execution -> evidence -> workflow"
    :modes
      ((mode human-interactive
         :requires [intent-review-gate plan-review-gate]
         :flow [request intent-alignment approve-intent plan approve-plan execute])
       (mode trusted-agent
         :requires [trusted-agent-policy risk-gate scoped-write-gate]
         :flow [request plan-with-intent policy-gate execute]
         :audit "intent alignment is embedded in plan.intent and preserved in lifecycle events"))
    :single-entry-surface mission_request
    :compat-surfaces [mission_directive mission_plan mission_workflow]
    :non-goal "Do not let clients bypass plan-runner by directly dispatching workstation work."
    (review-packet
      :desc "Compact projection of which artifact mission_request expects the human to review next; pure projection from request-local artifact existence + latest pipeline projection. Never auto-approves intent, never auto-dispatches plan."
      :surface mission_request
      :emitted-on [start advance status]
      :fields [:state :artifact_kind :artifact_path :artifact_exists
               :artifact_preview :prompt :allowed_responses :next_action
               :execute_allowed]
      :response-rule "start/advance/status/respond expose request-local :artifact_paths + :artifact_exists at the top level whenever a request_id can be resolved; callers do not need to inspect legacy pipeline file paths or nested wrappers to locate Lisp artifacts."
      :states [:received :intent_drafting :awaiting_intent_approval
               :awaiting_plan_approval :awaiting_execution :execute_requested]
      :state-derivation
        ((rule plan-present-wins
           :when "plan.lisp exists, execute was not explicitly requested, and the latest review event is not dispatched approve_plan"
           :state :awaiting_plan_approval
           :artifact_kind :plan
           :next_action "call mission_request respond with response=approve_plan / reject_plan / ask_question; execute later via response=execute_plan + execute=true"
           :execute_allowed false)
         (rule plan-present-execute-requested
           :when "plan.lisp exists and execute=true was passed on this call or the latest review event is dispatched execute_plan"
           :state :execute_requested
           :artifact_kind :plan
           :next_action "observe execution status through mission_request status and task receipts"
           :execute_allowed true)
         (rule plan-approved-event
           :when "plan.lisp exists and the latest review event is dispatched approve_plan"
           :state :awaiting_execution
           :artifact_kind :plan
           :next_action "call mission_request respond with response=execute_plan + execute=true"
           :execute_allowed true)
         (rule intent-only-present
           :when "intent-alignment.lisp exists and plan.lisp does not"
           :state :awaiting_intent_approval
           :artifact_kind :intent_alignment
           :next_action "call mission_request respond with response=approve_intent / reject_intent / ask_question"
           :execute_allowed false)
         (rule intent-drafting
           :when "neither intent-alignment.lisp nor plan.lisp exists, but projection just wrote one (target=intent_alignment|plan)"
           :state :intent_drafting
           :artifact_kind :intent_alignment
           :next_action "wait for projection to land, then re-poll mission_request status"
           :execute_allowed false)
         (rule received-default
           :when "no request-local artifacts and no projection target"
           :state :received
           :artifact_kind :request
           :next_action "call mission_request advance to drive the next pipeline stage"
           :execute_allowed false))
      :allowed-responses
        ((human-interactive
           :awaiting_intent_approval [approve_intent reject_intent ask_question]
           :awaiting_plan_approval [approve_plan reject_plan ask_question]
           :awaiting_execution [execute_plan ask_question]
           :execute_requested [observe]
           :default [observe])
         (trusted-agent
           :awaiting_intent_approval [approve_intent ask_question]
           :awaiting_plan_approval [approve_plan ask_question]
           :awaiting_execution [execute_plan ask_question]
           :execute_requested [observe]
           :default [observe]))
      :preview-policy
        (:source "request-local artifact bytes when artifact_exists; otherwise compiled_sexp_preview from latest projection"
         :max-bytes 480
         :truncation "missiond-core safe_byte_truncate (UTF-8 boundary safe)"
         :rationale "previews must never panic on multi-byte CJK runes")
      :non-goal "review_packet is pure observation; only an explicit review-response may call approval gates or plan-authoring, and no path dispatches workstation slots without execute_plan.")
    (review-response
      :desc "Caller continuation of a review_packet through mission_request. The user-facing surface answers a review_packet without learning the inner mission_directive / mission_plan calls; mission_request is the adapter that routes to the existing approval gates and never bypasses them or directly dispatches workstation work."
      :surface mission_request
      :action respond
      :inputs [:request_id :response :decision :note :board_task_id :execute
               :directive_id :approved_directive_id :directive_version
               :plan_id :approved_plan_id :project :cwd :target_project
               :target :dispatch_strategy :parallelism :objective :flow_id]
      :decisions [approve_intent reject_intent ask_question
                  approve_plan reject_plan execute_plan]
      :decision-routing
        ((rule approve-intent
           :requires [persisted-or-explicit-directive-ref]
           :route "delegate to mission_directive(action=approve, directive_id, version) using the existing approval gate; when approval succeeds, ensure a hidden BoardTask anchor if board_task_id was not supplied, then immediately continue through unified_entry s4 plan-authoring and project the resulting sexp into the same request-local plan.lisp; dry_run plan-authoring must include Lisp-native execution hints (:target, :objective, :nodes) so later execute_plan can route from plan.lisp rather than caller-supplied escape hatches"
           :default-board-task "board_task_id if supplied; otherwise create a hidden request-local BoardTask anchor so callers do not need internal board ids"
           :next_action "review request-local plan.lisp from the returned review_packet")
         (rule reject-intent
           :requires [persisted-or-explicit-directive-ref :note]
           :route "no DB mutation; record review event under .missiond/requests/<request_id>/events as auditable user decision"
           :next_action "revise the message and call mission_request start/advance again, or use mission_directive directly for explicit review_decision=rejected")
         (rule ask-question
           :requires [:note]
           :route "no DB mutation; record review event capturing the question text; orchestrator/UI surfaces it"
           :next_action "wait for follow-up answer, then call mission_request respond again with approve_intent / approve_plan")
         (rule approve-plan
           :requires [persisted-or-explicit-plan-ref request-local-plan-lisp]
           :route "when plan_id is explicit, parsed from plan.lisp, or recovered from a prior review event, delegate to mission_plan(action=approve, plan_id); when only request-local plan.lisp exists, materialize it into a draft Plan row first, reusing plan.lisp's BoardTask anchor when present or creating a hidden one only if needed, then approve through mission_plan; never sets execute=true"
           :next_action "call mission_request respond again with response=execute_plan + execute=true to dispatch the approved plan")
         (rule reject-plan
           :requires [persisted-or-explicit-plan-ref :note]
           :route "no DB mutation; record review event"
           :next_action "revise the plan and call mission_request advance, or use mission_plan directly for explicit review_decision=rejected")
         (rule execute-plan
           :requires [persisted-or-explicit-plan-ref :execute-true]
           :route "delegate to unified_entry::run_pipeline with approved_plan_id + execute=true so mission_plan execute path enforces the same scoped-write / risk gates"
           :guard "execute_plan requires execute=true (or response=execute_plan); a missing execute flag returns a structured blocked response, never a silent dispatch"))
      :ref-resolution
        (:order [explicit-arg artifact-extracted review-event-extracted request-local-materialized]
         :explicit-arg "callers may pass approved_directive_id / directive_id / approved_plan_id / plan_id directly"
         :artifact-extracted "request-local intent-alignment.lisp / plan.lisp is parsed for the persisted id when the explicit arg is omitted; artifact extraction trusts explicit :directive_id / :plan_id, and treats generic :id as a persisted directive/plan ref only when it is UUID-shaped so nested ids such as (:id \"root\") never become refs"
         :review-event-extracted "execute_plan can recover the plan_id from the latest request-local approve_plan review event"
         :request-local-materialized "approve_plan may materialize a persisted plan_id by inserting a draft Plan row from request-local plan.lisp, reusing its BoardTask anchor when present and creating a hidden request-local anchor only when needed; after materialization it writes the persisted ref back into request-local plan.lisp so the artifact is self-contained"
         :missing "when neither source yields or can materialize a persisted ref, return a structured blocked response with next_action describing how to obtain it; mission_request never fabricates non-persisted ids")
      :event-ledger
        (:path ".missiond/requests/<request_id>/events/<seq>.event.lisp"
         :schema "missiond.lifecycle-event.v1"
         :kinds [review_response_recorded review_response_dispatched review_response_blocked]
         :seq-allocation "monotonically increasing local sequence; allocator scans existing event files, picks max+1, and writes atomically — never overwrites an existing event"
         :writer "mission_request action=respond"
         :payload [:request_id :decision :note :directive_id :plan_id :execute :outcome])
      :response-shape
        (:respond_result {:decision :outcome :event_path :event_seq :next_action
                          :directive_id :plan_id :execute :inner_action :board_task_materialization :plan_materialization}
         :review_packet review-packet
         :projection "present on approve_intent when the follow-up plan compile projected plan.lisp"
         :board_task_materialization "present on approve_intent when request-local plan-authoring needed a hidden BoardTask anchor"
         :plan_materialization "present on approve_plan when request-local plan.lisp was promoted to a hidden BoardTask + draft Plan row"
         :pipeline_result "inner directive/plan/unified-entry payload when the route invoked one; approve_intent nests approval + plan_compile + projection; null for record-only routes"
         :next_action "human-readable continuation hint mirroring review_packet.next_action")
      :non-goals
        ("never auto-approves intent or plan when the user said reject/ask"
         "never spawns workstation work directly — execute_plan is a thin wrapper around mission_plan execute"
         "never invents a directive/plan id; missing-ref always returns blocked"
         "never edits the inner mission_directive / mission_plan / unified_entry handlers — adapter only"))
    (tool-schema-contract
      :surface mission_request
      :rule "The MCP input_schema is a projection of this Lisp review-response contract, not a permissive hidden bag; fields used for plan routing such as :target, :objective, :requested_cwd, :flow_id, :dispatch_strategy, :parallelism, :target_project, :cwd, :project, :execute_mode, :scheduler_mode, and :dry_run must be visible as explicit tool properties even when additionalProperties remains true for compatibility. The compatibility-writer switch :compat_write_file MUST be exposed as an explicit boolean property; legacy :write_file is preserved as an alias only."
      :implementation "crates/missiond-mcp/src/tools/knowledge/request.rs builds properties structurally to avoid serde_json::json! recursion limits as the Lisp contract grows.")
    (compat-writer-policy
      :surface mission_request
      :rule "Default mission_request flow projects only request-local artifacts (request.lisp, intent-alignment.lisp, plan.lisp, events/<seq>.event.lisp under .missiond/requests/<request_id>/). The legacy compatibility writers under .missiond/alignment/<topic>/ and .missiond/plans/<plan_id>/ MUST be opt-in: callers pass compat_write_file=true (V3 name) or legacy write_file=true (alias) to fire them. mission_request MUST NOT forward write_file=true to mission_directive or mission_plan unless one of those flags is explicitly true on the caller args."
      :v3-flag :compat_write_file
      :legacy-alias :write_file
      :default false
      :rationale "Wave43 evidence: live mission_request smoke that passed write_file=true left .missiond/alignment/<request_id>/ and .missiond/plans/<plan_id>/ artifacts in the worktree even after --cleanup, because the request-local cleanup scope intentionally excludes the compat roots. Defaulting compat off keeps the worktree request-local while preserving the legacy escape hatch for callers that depend on the old roots.")
    (execute-dry-run-smoke
      :surface mission_request
      :rule "Live mission_request smoke MUST keep the default --live-ipc path stopping at awaiting_execution; the only path that MAY call execute_plan from a checker is an explicit opt-in audit mode (preferred name --execute-dry-run on scripts/check-v3-request-flow-smoke.mjs). That audit path MUST drive the workstation-dispatch substrate end-to-end without spawning a slot: it MUST pass execute=true, dry_run=true, execute_mode=internal, dispatch_strategy=agent-team, and target=mission_task_delegate on the execute_plan respond call so mission_plan's `action_execute_internal` reaches `run_workstation_dispatch_with_contract_and_trace` and returns the `WorkstationDispatchOutcome::DryRun` shape: status=dry_run, execute_mode=internal, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dry_run_no_dispatch, target_tool=mission_task_delegate, dispatch_strategy=agent-team, with task_brief_preview present. Bridge mode (status=bridge_ready, runner_status=bridge_only) is no longer accepted as a no-dispatch proof for --execute-dry-run because it bypasses the substrate; the audit must prove MissionD reached the workstation-dispatch substrate but emitted would_dispatch instead of dispatching. The smoke MUST NOT spawn or wait for a ClaudeCode worker."
      :audit-flag :--execute-dry-run
      :respond-args (:response :execute_plan
                     :execute true
                     :dry_run true
                     :execute_mode :internal
                     :dispatch_strategy :agent-team
                     :target :mission_task_delegate)
      :asserts [:respond_outcome_dispatched
                :respond_inner_action_unified_entry_plan_execute
                :respond_result_execute_true
                :review_packet_state_execute_requested
                :allowed_responses_observe_only
                :request_local_execute_plan_event_appended
                :pipeline_result_no_dispatch_proof
                :pipeline_execute_mode_internal
                :pipeline_runner_status_workstation_dispatch_v0
                :pipeline_workstation_dispatch_status_dry_run_no_dispatch
                :pipeline_target_tool_mission_task_delegate
                :pipeline_dispatch_strategy_agent_team
                :pipeline_task_brief_preview_present]
      :no-dispatch-proofs ((workstation-dispatch-substrate
                             :status "dry_run"
                             :execute_mode "internal"
                             :runner_status "workstation_dispatch_v0"
                             :workstation_dispatch_status "dry_run_no_dispatch"
                             :target_tool "mission_task_delegate"
                             :dispatch_strategy "agent-team"
                             :task_brief_preview :present))
      :non-goal "Default --live-ipc and the v3 aggregate gate MUST remain non-executing; the audit flag is opt-in for explicit smoke runs and never appears in check-v3-code-isomorphism-complete."
      :rationale "Wave45 proved mission_request can drive execute_plan without consuming a workstation slot, but the observed no-dispatch proof was bridge mode (status=bridge_ready / runner_status=bridge_only) — bridge mode short-circuits before the workstation_dispatch substrate runs, so it does not exercise `run_workstation_dispatch_with_contract_and_trace`, evidence emission, or task_brief rendering. Wave46 tightens the audit so the smoke explicitly drives execute_mode=internal + dispatch_strategy=agent-team, satisfying `evaluate_dispatch_decision`'s auto-inference (target=mission_task_delegate + INFERABLE strategy + non-empty objective + cwd as scoping signal) and forcing the substrate path. The expected outcome `WorkstationDispatchOutcome::DryRun` builds the brief, skips the inner tool, and returns `workstation_dispatch_status=dry_run_no_dispatch` with `task_brief_preview` populated. This proves MissionD reached the workstation-dispatch substrate without spawning a slot.")
    (real-dispatch-smoke
      :surface mission_request
      :rule "Real dispatch through mission_request execute_plan is slow + side-effecting (it creates a delegated BoardTask and may auto-provision a worker slot via mission_task_delegate). It MUST stay behind a separate, deliberately named opt-in flag (preferred name --execute-real-dispatch on scripts/check-v3-request-flow-smoke.mjs) and MUST NOT appear in default --live-ipc, --execute-dry-run, or check-v3-code-isomorphism-complete. The opt-in audit MUST pass execute=true, dry_run=false (or omit dry_run), execute_mode=internal, dispatch_strategy=agent-team, target=mission_task_delegate, cwd=<repo>, and a smoke objective that explicitly tells the delegated worker to do no file edits and no commits (read-only smoke; classify_task_kind→ReadOnly with empty owned_files so the brief instructs commit_status=not-required). The substrate (run_workstation_dispatch_with_contract_and_trace) MUST take the `WorkstationDispatchOutcome::Dispatched` branch and the response MUST surface: pipeline_result.status=executing (the plan FSM transitions to Executing on a successful Dispatched outcome — see plan.rs::build_workstation_dispatch_response), execute_mode=internal, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dispatched (the substrate-level dispatch invariant emitted by outcome_to_response_fields), target_tool=mission_task_delegate, dispatch_strategy=agent-team, task_brief_preview present (non-empty), inner_result present and non-null, and a stable delegated BoardTask UUID at pipeline_result.delegated_board_task_id (projected by workstation_dispatch/outcome.rs::extract_inner_board_task_id from the inner mission_task_delegate response, which currently embeds the full BoardTask row at inner_result.task_id because compute/task_delegate.rs::handle shadows the variable name). The smoke MUST NOT wait synchronously for the delegated worker to finish; if a wait/observe mode is offered it MUST be a SECOND, separately gated, bounded option (not the default of --execute-real-dispatch). Filesystem cleanup is request-local only: --cleanup may remove .missiond/requests/<request_id>/ but MUST NOT delete the delegated BoardTask row, audit rows, or any worker-side artifacts. The checker MUST report delegated_board_task_id and the observed BoardTask status so the parent / Autopilot can observe or close the BoardTask."
      :completion-log-rule "Live workstation dispatch MUST pre-open the companion MissionD execution log before mission_task_delegate receives the brief. The brief pins `execution_id=\"plan-<plan_id>\"`, and read-only workers may append only that audit log when calling mission_execution(action=complete); if the log cannot be opened, dispatch returns skipped_completion_log_unavailable instead of handing the worker an impossible completion contract."
      :audit-flag :--execute-real-dispatch
      :respond-args (:response :execute_plan
                     :execute true
                     :dry_run false
                     :execute_mode :internal
                     :dispatch_strategy :agent-team
                     :target :mission_task_delegate
                     :cwd :repo-root
                     :objective :no-edit-no-commit-smoke)
      :asserts [:respond_outcome_dispatched
                :respond_inner_action_unified_entry_plan_execute
                :respond_result_execute_true
                :review_packet_state_execute_requested
                :allowed_responses_observe_only
                :request_local_execute_plan_event_appended
                :pipeline_status_executing
                :pipeline_execute_mode_internal
                :pipeline_runner_status_workstation_dispatch_v0
                :pipeline_workstation_dispatch_status_dispatched
                :pipeline_target_tool_mission_task_delegate
                :pipeline_dispatch_strategy_agent_team
                :pipeline_task_brief_preview_present
                :pipeline_inner_result_present
                :pipeline_delegated_board_task_id_uuid]
      :dispatch-proof ((workstation-dispatch-substrate
                         :status "executing"
                         :execute_mode "internal"
                         :runner_status "workstation_dispatch_v0"
                         :workstation_dispatch_status "dispatched"
                         :target_tool "mission_task_delegate"
                         :dispatch_strategy "agent-team"
                         :task_brief_preview :present
                         :inner_result :present
                         :delegated_board_task_id :uuid))
      :rust-projection-source "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/outcome.rs::extract_inner_board_task_id"
      :non-goal "Default --live-ipc, --execute-dry-run, and check-v3-code-isomorphism-complete MUST remain non-real-dispatching. --execute-real-dispatch is the SOLE entry point that creates a delegated BoardTask + (optionally) auto-provisions a worker slot. CI MUST NOT block on the delegated worker finishing — the smoke validates creation/response shape only and leaves the BoardTask for Autopilot to drive."
      :rationale "Wave46 proved the workstation_dispatch substrate accepts execute_mode=internal + dry_run=true and returns `WorkstationDispatchOutcome::DryRun` with workstation_dispatch_status=dry_run_no_dispatch — the LAST step before a real dispatch. Wave47 closes the loop by exercising the same substrate with dry_run=false: `run_workstation_dispatch_with_contract_and_trace` builds the brief, calls mission_task_delegate (which auto-creates a BoardTask via state.store.create_board_task and notifies the dispatcher), and returns `WorkstationDispatchOutcome::Dispatched { inner_payload, .. }`. The minimal Rust projection (extract_inner_board_task_id) surfaces a stable `delegated_board_task_id` UUID at the top level of pipeline_result without rewriting compute/task_delegate.rs (which is outside this wave's write scope). Two deviations from the brief's draft assertions, both documented above and reflecting the established daemon shape: (a) pipeline_result.status='executing' (NOT 'dispatched') because the wave-15 substrate response intentionally surfaces the FSM transition; the substrate-level dispatch invariant lives at workstation_dispatch_status='dispatched'. (b) the BoardTask UUID is exposed via the new top-level `delegated_board_task_id` field rather than `inner_result.task_id` (which today contains the full DB row, not a string). This is intentionally the only checker that may consume a real workstation slot; gating it behind a deliberately-named opt-in flag (rather than overloading --confirm-execute) prevents accidental real dispatch from CI or daemon-free runs."))

  (state-machines
    (state-machine unified-entry
      :initial :received
      :terminal [:done :failed :blocked]
      (transition :received -> :intent_drafted
        :actor alignment-author
        :writes [intent-alignment]
        :when "mode=human-interactive")
      (transition :intent_drafted -> :awaiting_intent_approval
        :actor orchestrator
        :emits [:review_question_created])
      (transition :awaiting_intent_approval -> :intent_approved
        :gate intent-review-gate)
      (transition :intent_approved -> :plan_drafted
        :actor plan-author
        :writes [plan])
      (transition :plan_drafted -> :awaiting_plan_approval
        :actor orchestrator
        :emits [:review_question_created])
      (transition :awaiting_plan_approval -> :plan_approved
        :gate plan-review-gate)
      (transition :received -> :plan_drafted
        :actor trusted-agent
        :writes [plan]
        :when "mode=trusted-agent and policy allows folded intent")
      (transition :plan_approved -> :executing
        :actor plan-runner
        :writes [lifecycle-event])
      (transition :executing -> :verifying
        :actor verifier
        :writes [verification-receipt])
      (transition :verifying -> :done
        :actor finalizer
        :writes [final-report]))

    (state-machine execution-lifecycle
      :initial :planned
      :terminal [:complete :failed :abandoned :superseded]
      :derived-from lifecycle-event
      :states [:planned :dispatchable :dispatched :claimed :running
               :worker_committed :draft_reported :parent_patched
               :verification_pending :verified :report_finalized :complete
               :blocked :stale :failed :abandoned :superseded]
      :completion-rule "complete iff final-report is finalized, final commit matches lineage, and required receipts are valid")

    (state-machine delegated-boardtask-runtime
      :initial :queued
      :terminal [:done :blocked :failed :skipped]
      :derived-from [BoardEvent SlotEvent ExecutionEvent]
      :states [:queued :event_woken :eligible :claimed :slot_selected
               :prompt_sent :running :completed :completion_audited
               :done :blocked :failed :skipped]
      (transition :queued -> :event_woken
        :event [BoardEvent::TaskCreated SlotEvent::BecameIdle]
        :actor event-bus-subscriber
        :effect "notify dedicated Autopilot dispatch task without running pty.send inline")
      (transition :event_woken -> :eligible
        :actor autopilot-runtime
        :reads [board_task dependency_state slot_state global_pause])
      (transition :eligible -> :claimed
        :actor autopilot-runtime
        :writes [board_claim lease])
      (transition :claimed -> :slot_selected
        :actor autopilot-runtime
        :writes [assignee dispatch_guard])
      (transition :slot_selected -> :prompt_sent
        :actor autopilot-runtime
        :emits [SlotEvent::TaskDispatched])
      (transition :prompt_sent -> :running
        :actor worker-slot)
      (transition :running -> :completed
        :actor worker-slot
        :emits [ExecutionEvent::Completed])
      (transition :completed -> :completion_audited
        :actor autopilot-runtime
        :writes [mission_execution completion-note])
      (transition :completion_audited -> :done
        :actor autopilot-runtime
        :writes [BoardEvent::StatusChanged])
      :completion-rule "A delegated BoardTask is complete only after Autopilot observes worker completion or self-close, reconciles mission_execution completion, and emits the final BoardEvent status transition."))

  (policies
    (policy risk-gate
      :inputs [mode objective write_scope must_not_touch risk_level external_side_effects]
      :allow-auto-approval-if
        [:trusted_agent :low_or_medium_risk :bounded_write_scope
         :no_destructive_action :acceptance_present :rollback_or_blocker_present]
      :must-ask-human-if
        [:high_risk :ambiguous_goal :destructive_action :external_publish
         :payment_or_secret :unbounded_write_scope])

    (policy scoped-write-gate
      :inputs [plan.nodes.write_scope plan.nodes.must_not_touch git_status]
      :checks [:owned_paths_only :forbidden_paths_empty :nul_free :diff_check_clean])

    (policy parent-hotfix-finalization
      :rule "Parent patches after worker exit must append events and regenerate final-report lineage.")

    (policy verification-reuse
      :rule "Receipts may cover later states only when commit prefix, file set, tier, and exit_code rules pass."))

  (source-hygiene
    :desc "Read-only source and staged-index hygiene before scoped task commits."
    :entrypoints [scripts/check-staged-source-hygiene.mjs
                  scripts/task-scope-guard.mjs
                  .githooks/pre-commit]
    :hook-policy "Repo-local hook install is explicit opt-in; the pre-commit hook is a no-op unless MISSIOND_TASK_CONTRACT names a task.lisp contract."
    :invariants
      ["Staged hygiene MUST be read-only: no git add, commit, reset, checkout, stash, push, merge, rebase, hook mutation, or working-tree mutation."
       "MISSIOND_TASK_CONTRACT enables task-scope guard enforcement in the pre-commit hook; without it the hook exits 0 so non-task commits are not blocked."
       "Staged source hygiene MUST reject raw NUL bytes in staged blobs before commit."
       "Staged source hygiene MUST run git diff --cached --check over the staged path set."
       "Task-scope guard MUST reject staged paths outside :write-scope and any path matching :must-not-touch."
       "Repo text search hygiene MUST project ssot-retrieval-scope into .ignore sidecars so searches rooted at repo, .missiond, .missiond/v3, .missiond/research, or .missiond/tasks preserve the active-authoring default."
       "Default repo rg MUST NOT surface .missiond/research/true-user-utterances-*.md, archived session dumps, imported transcript exports, or .missiond/v3/runtime cold compiled projections; explicit --no-ignore or explicit path remains the historical forensics entry."
       "The hook doctor MUST be read-only by default; hook installation is a separate explicit install command."
       "Batch verification MAY import checkSuppliedFiles for final-tree source hygiene fixtures, but must not mutate git."])

  (multi-agent-context-pack
    :desc "Two-stage parallel investigation and shard implementation as a Lisp-owned append-only context bus."
    :schema "missiond.context-pack.v1"
    :write-model "multi-agent append-only"
    :entry-heads [claim observation anchor shard-proposal conflict integration-plan]
    :mutation-owner "append helper / writer-specific entry only; no worker rewrites prior entries"
    :merge-owner "orchestrator or context-integrator appends a single integration-plan after reading proposals"
    :flow [parallel-claims parallel-observations shard-proposals conflict-notes integration-plan compile-shards materialize-wave run-wave dispatch-code-workers verify-and-finalize]
    :roles
      ((context-investigator :writes [claim observation anchor shard-proposal conflict] :forbidden [code-edits commits])
       (context-integrator :writes [integration-plan] :reads [shard-proposal conflict])
       (code-worker :reads [integration-plan accepted-shards dispatch-groups] :writes [declared-shard-write-scope report commit])
       (parent-verifier :writes [verification-receipt final-report parent-patches]))
    :invariants
      ["Context investigators MAY run concurrently and append claim/observation/anchor/shard-proposal/conflict entries to the same context-pack.lisp."
       "Every entry MUST carry :id :agent :seq :at; :seq is strictly increasing and allocated by the append path, not guessed from stale reads."
       "shard-proposal entries MUST declare :shard :owner :write-scope :must-not-touch :acceptance so code workers can execute without re-deriving architecture."
       "integration-plan MUST cite accepted-shards and dispatch-groups; mapped dispatch groups SHOULD use (group :id <id> :shards [...]) so orchestration can compile code-worker waves without narrative parsing."
       "context-pack-materialize-wave MUST refuse names-only dispatch groups and may only project mapped integration-plan shards into task-runner manifest + task-contract files."
       "context-pack-run-wave is the single orchestration entry from context-pack SSOT to prepared task-runner wave and dispatch descriptor; it must not submit workers unless --apply is explicit."
       "context-pack-run-wave MUST create missing shared-memory/session-trace ledgers with create-only semantics before prepare-task-runner-wave, and MUST NOT rewrite existing ledgers."
       "Accepted shard write-scope entries MUST NOT overlap unless a later conflict entry explicitly routes that hotspot to a single owner."
       "Context pack writers produce evidence and proposals only; code implementation happens in later shard tasks with disjoint write scopes."
       "code workers consume the latest integration-plan through context-pack-compile-shards; they do not reinterpret investigator observations as authority."
       "Shared-memory remains coarse lifecycle memory; context-pack is the high-density planning surface that turns concurrent investigation into implementable shards."]
    :checker "node scripts/check-context-pack.mjs")
