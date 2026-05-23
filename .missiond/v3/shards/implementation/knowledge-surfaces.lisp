(implementation-map
(surface mission-shared-memory
      :status "code-aligned"
      :implements [shared-events shared-artifacts shared-claims agent-cursors context-slices evidence-governance-view task-delegate-write-lease swarm-write-lease]
      :code ["crates/missiond-core/migrations/20260508000000_shared_memory.sql"
             "crates/missiond-daemon/src/engine/shared_memory.rs"
             "crates/missiond-daemon/src/state.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
             "crates/missiond-mcp/src/tools/mod.rs"
             "scripts/check-v3-shared-memory-isomorphism.mjs"
             ".missiond/workflows/semantic-ir-shared-memory-convergence.lisp"]
      :note "MissionD shared memory is the Rust/Postgres durable coordination substrate for concurrent agents. EventBus wakes and observes; shared_events/shared_artifacts/shared_claims/agent_cursors hold the coordination truth. Investigation workers write artifacts without claims; implementation workers must have an accepted shard and write-scope lease. mission_shared_memory(action=evidence_view) projects the unified Memory/KB + Logs/Timeline/Conversation model: task_result_artifacts are canonical worker outputs, conversations are provider/user turn read models, event_log/shared_events are causality, KB is reviewed long-term knowledge, and BoardTask state is coordination projection. Legacy shared-memory.lisp ledgers remain compatibility projections, not concurrent write authority.")

(surface evidence-governance-view
      :status "code-aligned"
      :implements [evidence-governance-view task-result-artifact-authority conversation-read-model timeline-causality-view reviewed-kb-memory board-coordination-projection]
      :code ["crates/missiond-daemon/src/engine/shared_memory.rs"
             "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
             "scripts/check-v3-shared-memory-isomorphism.mjs"
             ".missiond/v3/missiond-blueprint.lisp"]
      :acceptance ["node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                   "node scripts/check-v3-pillar-flow-schema.mjs --engine=ocaml --json"]
      :note "evidence-governance-view is the unified read surface that prevents Memory/KB, Logs, Timeline, Conversation, and Board from competing as result authorities. It is served through mission_shared_memory(action=evidence_view), but it remains a distinct SSOT surface so pillar-flow keeps a single function-to-surface mapping.")

(surface context-pack
      :status "code-aligned"
      :implements [multi-agent-context-pack]
      :code ["scripts/check-context-pack.mjs"
             "scripts/context-pack-append.mjs"
             "scripts/context-pack-compile-shards.mjs"
             "scripts/context-pack-materialize-wave.mjs"
             "scripts/context-pack-run-wave.mjs"
             "scripts/lib/v3_workstation_runtime.mjs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "scripts/check-v3-context-pack-isomorphism.mjs"]
      :note "Context-pack is the V3 high-density planning surface for two-stage parallel work: context investigators append claim/observation/anchor/shard-proposal/conflict entries to .missiond/tasks/<wave>/context-pack.lisp without code edits, then an orchestrator/integrator appends integration-plan with accepted-shards and dispatch-groups. Mapped dispatch groups use (group :id <id> :shards [...]) so scripts/context-pack-compile-shards.mjs can project the Lisp plan into dispatchable_groups for code workers; legacy bare group ids remain names_only for older packs. mission_swarm_run materializes a lightweight missiond.swarm-context-pack.v1 sidecar before publishing worker BoardTasks so provider workers can read the declared context_pack_path."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-011")

(surface memory-kb
      :status "code-aligned"
      :implements [knowledge-memory kb-manager memory insight intent-snapshot]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/args.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/remember.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/quality.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/compact.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/analyze.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/mutate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/import.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/gc.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/ops.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/beacon.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/code_search.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
             "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
             "crates/missiond-core/migrations/20260508001000_knowledge_review_state.sql"
             "crates/missiond-core/src/types/knowledge.rs"
             "crates/missiond-core/src/db/traits.rs"
             "crates/missiond-core/src/db/pg/knowledge.rs"
             "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
             "crates/missiond-daemon/src/engine/learning_engine/mod.rs"
             "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
             "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
             "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
             "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
             "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
             "crates/missiond-core/src/db/pg/conversation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/insight.rs"
             "crates/missiond-daemon/src/handlers/knowledge/intent.rs"
             "crates/missiond-mcp/src/tools/knowledge/kb.rs"
             "crates/missiond-mcp/src/tools/knowledge/memory.rs"
             "crates/missiond-mcp/src/tools/knowledge/insight.rs"
             "crates/missiond-mcp/src/tools/knowledge/intent.rs"
             "scripts/check-v3-memory-kb-isomorphism.mjs"]
	      :note "Runtime-projected V3 destination for memory/KB tools. memory-kb-policy and learning-engine-policy own budgets, cadences, bounded SQL probes, shared KB dedupe gate semantics, and physical split ownership across kb/* modules. KbStore::kb_remember is the shared write gate for realtime/deep-analysis/manual/internal memory writes; same-session duplicates preserve evidence_refs/source_sessions/superseded_by provenance instead of creating two active keys. kb/review.rs owns non-destructive knowledge_review_state overlay so stale memories leave default retrieval without deleting evidence; low-confidence semantic duplicates become needs-human review artifacts. Conversation history distillation remains deferred behind .missiond/workflows/conversation-memory-distillation.lisp."
	      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-015")

(surface codex-boot-context
      :status "code-aligned"
      :implements [codex-boot-context mission_context_boot boot-capsule external-chat-handoff]
      :code [".missiond/v3/evidence/codex-boot-context.lisp"
             "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
             "scripts/check-v3-codex-boot-context-isomorphism.mjs"]
      :acceptance ["node scripts/check-v3-codex-boot-context-isomorphism.mjs --json"]
      :note "codex-boot-context is the small versioned startup capsule for resident Codex, Codex workers, and external Codex handoffs. It carries only collaboration protocol and layer rules; task/project facts must be gathered through mission_context_gather and cold evidence stays explicit. This lets new conversations start with MissionD's learned operating contract without injecting raw chat history, secrets, provider logs, or unreviewed KB.")

(surface project-registry
      :status "code-aligned"
      :implements [project-registry project-root-resolution service-runtime-universe]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/universe.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/reconcile.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/survey.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/vault.rs"
             "crates/missiond-core/src/types/project.rs"
             "crates/missiond-daemon/src/slot_orchestrator/project_root.rs"
             "crates/missiond-mcp/src/tools/knowledge/project.rs"
             "scripts/check-v3-project-registry-isomorphism.mjs"
             "scripts/check-project-maturity.mjs"]
      :note "Code-aligned destination for project registry/root resolution. project.rs is the mission_project facade; project/registry.rs owns list/get/set_active/sync/init/import_universe; project/universe.rs owns mission_project(action=universe) and projects service-runtime-universe entries such as auth production domain/deployment/DNS capability to master, workers, and Board System. ProjectRegistry::resolve owns longest-prefix project lookup; inactive project aliases never participate in cwd resolution, and mission_project init archives inactive path aliases before upsert so stale aliases cannot block canonical root correction. check-project-maturity.mjs is the project-universe maturity gate: --min-level M5 proves worker-operational SSOT closure; --min-level M6 proves Auth-grade domain/policy/flow/event/runtime/compatibility depth. It resolves the MissionD blueprint from the checker script directory so external-project workers can run it from the target root."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-016")

(surface data-residency-universe
      :status "code-aligned"
      :implements [data-residency-universe data-region-partition-contract xjp-platform-partition-contract cross-region-data-policy project-region-declaration]
      :code [".missiond/v3/missiond-blueprint.lisp"
             ".missiond/research/data-residency-universe-report-20260512.md"
             "scripts/check-v3-data-residency-universe-isomorphism.mjs"
             "scripts/check-project-ssot-universe.mjs"
             "scripts/check-project-maturity.mjs"
             "/Users/jinchen/Downloads/PCEA develop/.missiond/intent.lisp"
             "/Users/jinchen/Downloads/PCEA develop/.missiond/check.sh"]
      :note "data-residency-universe is the SSOT surface for cn/global hard partitions and global-eu operating-zone policy. It now models XJP platform partitions first: xjp-cn owns the Aliyun ECS/CN infra stack, xjp-global owns the GCP/global stack, and app-level partitions such as pcea-cn/pcea-global or cuthub-cn/cuthub-global bind to those stacks. It keeps region identity, issuer, secret, storage, payment ledger, model router, event, deploy, and cross-region egress rules out of ad hoc deployment notes. PCEA's project-local .missiond/check.sh pins the same declarations so data-bearing M6 means platform-partition-aware, not merely code-mapped.")

(surface memory-provider-boundary
      :status "code-aligned"
      :implements [memory-provider-contract memory-provider]
      :code [".missiond/research/missiond-memory-eventhub-modularization-20260512.md"
             ".missiond/workflows/missiond-module-extraction.lisp"
             ".missiond/v3/missiond-blueprint.lisp"
             "scripts/manage-local-providers.sh"
             "scripts/check-v3-service-extraction-isomorphism.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "Pinned modularization decision: MissionD Core remains local orchestrator and long-term memory becomes a MemoryProvider contract with null/local/xjp provider implementations. mission_kb_* remains a compatibility facade; providers own conversation stores, active memory, review overlay, skill evidence, FTS, embedding, rerank, export, purge, and tenant/universe/project/user isolation.")

(surface eventhub-service-boundary
      :status "code-aligned"
      :implements [eventhub-service-contract eventhub-service]
      :code [".missiond/research/missiond-memory-eventhub-modularization-20260512.md"
             ".missiond/workflows/missiond-module-extraction.lisp"
             ".missiond/v3/missiond-blueprint.lisp"
             "scripts/manage-local-providers.sh"
             "scripts/check-v3-service-extraction-isomorphism.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "Pinned modularization decision: cross-service durable events move toward xjp-eventhub while MissionD keeps local EventBus and outbound spool for offline-safe agent orchestration. xjp-eventhub owns durable streams, cursors, subscriptions, waits, dead-letter, and replay for deploy-center/auth/router/timeline/service events.")

(surface router-policy
      :status "code-aligned"
      :implements [router-policy-dry-run router-backend-readiness router-dispatch-descriptor]
      :code ["crates/missiond-mcp/src/tools/comm/router_chat.rs"
             "crates/missiond-daemon/src/handlers/comm/router_chat.rs"
             "crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs"
             "crates/missiond-daemon/src/handlers/comm/router_chat/files.rs"
             "crates/missiond-daemon/src/handlers/comm/router_chat/manage.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
             "crates/missiond-daemon/src/llm/gemini_client.rs"
             "crates/missiond-daemon/src/llm/gemini_cli.rs"
             "crates/missiond-daemon/src/llm/gemini_driver.rs"
             "crates/missiond-daemon/src/llm/gemini_file_api.rs"
             "crates/missiond-daemon/src/llm/llm_gateway.rs"
             "crates/missiond-daemon/src/llm/sonnet_gateway.rs"
             "crates/missiond-daemon/src/llm/xjp_router_client.rs"
             "crates/missiond-daemon/src/infra/message_handler.rs"
             "crates/missiond-core/src/db/pg/observability.rs"
             "crates/missiond-core/src/ws/server.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs"
             "scripts/check-router-policy.mjs"
             "scripts/check-router-backend-registry.mjs"
             "scripts/check-router-dispatch-descriptor.mjs"
             "scripts/check-v3-router-policy-isomorphism.mjs"]
      :note "Runtime-projected V3 destination for the V2 router-policy dry-run chain and public router chat tools. router-runtime-policy owns chat/flow/Sonnet defaults, queue/tool/file timeouts, xjp-router embedding timeout, and token/compression budgets through RouterRuntimeConfig. router_chat/chat.rs owns mission_router_chat request normalization, LLM dispatch, persistence, and response projection; proxy mode carries the direct transcript prompt without sending /clear to shared PTY. Jarvis stream governance MUST write provider usage into token_usage_ledger through message_handler.rs and observability.rs so billing is a durable ledger. router_chat/files.rs owns attachment denylist; router_chat/manage.rs owns history/list/delete/clear/stats/compress."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-018")

(surface incident-governance
      :status "code-aligned"
      :implements [question incident llm-trace decision-stats auth]
      :code ["crates/missiond-daemon/src/handlers/comm/question.rs"
             "crates/missiond-daemon/src/handlers/comm/question/question_flow.rs"
             "crates/missiond-daemon/src/handlers/comm/question/decision.rs"
             "crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs"
             "crates/missiond-daemon/src/handlers/comm/question/auth.rs"
             "crates/missiond-daemon/src/handlers/comm/question/incident.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-mcp/src/tools/comm/question.rs"
             "scripts/check-v3-incident-governance-isomorphism.mjs"]
      :note "Code-aligned V3 destination for question, incident, LLM trace, Gemini auth, and decision stats behavior. question.rs is the thin incident-governance facade; question/question_flow.rs owns mission_question create/list/get/answer/dismiss, running-autopilot task inference, QuestionEvent::Created/Resolved, and TaskEvent::Completed scheduler wakeup; question/decision.rs owns mission_decision_stats; question/llm_trace.rs owns mission_llm_trace plus legacy Gemini/Jarvis trace aliases, Gemini request log/stat/content reads, and Gemini watch lifecycle, with the watch probe model projected from router-runtime-policy flow-gemini-model through RouterRuntimeConfig; question/auth.rs owns mission_gemini_auth llm.yaml/settings.json projection; question/incident.rs owns mission_incident routing plus legacy mission_incident_* execution, incident injection/list/get/remediate/status/close, triage remediations, and safe close audit; handlers/mod.rs sends consolidated and legacy question/incident/LLM trace public tools through this facade.")

(surface decision-inbox-revalidation
      :status "code-aligned"
      :implements [decision-inbox-revalidation stale-decision-revalidation]
      :code [".missiond/v3/missiond-blueprint.lisp"
             ".missiond/frontend/board-blueprint.lisp"
             "crates/missiond-daemon/src/handlers/comm/question/question_flow.rs"
             "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
             "packages/board/src/components/PendingQuestions.tsx"
             "packages/board/src/types.ts"
             "scripts/check-v3-lisp-code-sync-isomorphism.mjs"]
      :note "decision-inbox-revalidation makes operational questions revalidate their facts before reaching the user. mission_question list/get can classify stale lisp-code-sync self-loop questions, read authoritative runtime status such as reportDirs and recentReports5m, answer obsolete items as stale_evidence/resolved_by_runtime_fix, close the linked stale operational BoardTask as done instead of reopening it, emit QuestionEvent::Resolved, and return revalidationStatus/evidenceFreshAt fields for still-valid questions. The Board frontend displays this status instead of treating old incident text as fresh truth.")

(surface capability-governance
      :status "code-aligned"
      :implements [capability-usage audit codex-ops mcp-tool-governance]
      :code ["crates/missiond-mcp/src/tools/comm/capability_usage.rs"
             "crates/missiond-mcp/src/tools/comm/audit.rs"
             "crates/missiond-mcp/src/tools/comm/codex_ops.rs"
             "crates/missiond-mcp/src/tools/comm/tool_directory.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage/runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/audit.rs"
             "crates/missiond-daemon/src/handlers/comm/codex_ops.rs"
             "crates/missiond-daemon/src/handlers/comm/tool_directory.rs"
             "scripts/check-v3-capability-governance-isomorphism.mjs"]
      :note "Runtime-projected V3 destination for capability usage, audit, Codex ops, and MCP tool-family governance surfaces. capability_usage.rs is the thin capability-governance facade; capability_usage/runtime.rs owns snapshot/report/candidates/mark/ack, six source lanes, semantic hint merge review, protected source/target policy, review sidecar persistence, and non-blocking observability emissions; context/v3_blueprint_runtime.rs projects capability-governance-policy review sidecar path plus protected source/target lists into mission_capability_usage runtime; audit.rs owns mission_audit trace/detail/stats/export plus legacy mission_audit_* compatibility; codex_ops.rs owns mission_codex_ops recent/thread/tool_stats over codex_cli conversations; tool_directory.rs owns mission_tool_directory list/recommend/lookup/explain/deprecated/guide so agents can select primary families and task entry cards before raw compatibility tools.")

(surface skill-runtime
      :status "code-aligned"
      :implements [skill-query skill-context skill-operational-facts skill-mutate skill-exec]
      :code ["crates/missiond-daemon/src/handlers/knowledge/skill.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/mutate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/exec.rs"
             "crates/missiond-mcp/src/tools/knowledge/skill.rs"
             "scripts/check-v3-skill-runtime-isomorphism.mjs"]
      :note "skill-runtime is the code-aligned surface for skill query/context/mutate/exec. skill.rs is the thin facade; query/context/mutate/exec modules own FTS/vector lookup, project skill links, opt-in KB aggregation, skill-derived operational_facts, mutation rollback/materialization, and explicit approve=true replay for requires_approval workflows."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-022")

(surface cascade-governance
      :status "code-aligned"
      :implements [universe-graph cascade-plan cascade-trigger cascade-lint]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/path.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/graph.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/trigger.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/lint.rs"
             "crates/missiond-mcp/src/tools/knowledge/cascade.rs"
             "scripts/check-v3-cascade-governance-isomorphism.mjs"]
      :note "Code-aligned V3 destination for universe graph and cascade tools. cascade.rs is the thin cascade-governance facade; cascade/path.rs owns manifest/root path policy by loading CascadeRuntimeConfig from V3 cascade-policy before honoring explicit UNIVERSE_MANIFEST / UNIVERSE_ROOT overrides; cascade/graph.rs owns mission_universe_graph; cascade/plan.rs owns mission_cascade_plan dry-run; cascade/trigger.rs owns mission_cascade_trigger, V3 trigger-enabled plus CASCADE_TRIGGER_ENABLED explicit override, TaskEvent::CascadeTriggered/Completed, max-cycle clamp, and spawn_blocking execute_plan; cascade/lint.rs owns mission_cascade_lint integrity egress.")
)
