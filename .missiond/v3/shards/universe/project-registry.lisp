(project-registry-policy
    :desc "Lisp-owned project registry defaults for intent discovery and universe import."
    :intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
    :default-universe-manifest "$MISSIOND_PROJECTS_DIR/universe.intent.lisp"
    :env-overrides [UNIVERSE_MANIFEST]
    :invariants
      ["mission_project init/import_universe/survey MUST project intent-path candidates from project-registry-policy."
       "mission_project import_universe MUST project its default manifest from project-registry-policy; UNIVERSE_MANIFEST is only an explicit override."
       "A real MissionD project with .missiond but no project-registry-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

(project-discovery-contract
    :schema "missiond.project-discovery-contract.v1"
    :entrypoint mission_project.resolve
    :resolver-statuses [resolved ambiguous unregistered_candidate not_found stale_runtime]
    :lookup-sources [missiond-db compiled-project-universe service-runtime-universe cwd-root-prefix explicit-domain explicit-url unregistered-root-candidate]
    :compiled-universe-fields [aliases service_ids domains public_base_url frontend_url api_base_url domain_management]
    :rule "External agents MUST resolve project identity from names, aliases, domains, URLs, and cwd before querying KB, Board, conversations, SSOT summaries, or dispatching workers."
    :result-contract
      [:status :query :normalized :matched_project_id :matched_project :candidate_projects :candidate_roots :registration_proposal :diagnostics :next_actions]
    :invariants
      ["mission_project resolve is read-only and MUST NOT register or mutate projects."
       "Exact project id/domain/alias matches outrank fuzzy path or conversation evidence."
       "Compiled project-universe MUST expose aliases and service-runtime domains/URLs so ClaudeCode and Codex do not need ad-hoc filesystem grep to identify projects."
       "Agents that answer domain, DNS, subdomain, Cloudflare, Aliyun DNS, certificate, or public URL questions from MissionD project management MUST consult compiled domain_management and xjp-domain-service before reporting authority or mutating DNS."
       "mission_project get MUST accept id/project/project_id/projectId, enrich DB-backed and compiled-only projects with compiledProject/serviceRuntime/supportCatalog when available, and expose explicit db_status plus compiled_status so get does not contradict resolve."
       "If compiled project-universe is stale or unavailable, resolve MUST return a structured stale_runtime diagnostic and continue with DB/explicit query facts instead of hard-failing ordinary project discovery."
       "Unknown domains such as a new product site MUST return unregistered_candidate with a registration_proposal rather than treating the project as absent."
       "mission_context_gather MUST call mission_project resolve when query text is present and no explicit project_id is supplied."])

(project-identity-contract
    :schema "missiond.project-identity-contract.v1"
    :fields [project_id canonical_root repo_remote ssot_paths deploy_center_slug forge_project_name service_ids aliases status management_domain runtime_layer]
    :rule "MissionD is project identity and SSOT registry authority; deploy-center is deployment fact authority; Forge is component/pattern/reality catalog authority."
    :reconcile-action mission_project.reconcile
    :invariants
      ["MissionD Universe owns canonical project ids, roots, SSOT paths, maturity, Board links, and workstation dispatch."
       "deploy-center owns deployment targets, runtime location, release provenance, deploy agents, and executor state."
       "Forge owns component/pattern catalog, code reality mirror, and Universe DAG recommendations; Forge-only references are not deployable unless MissionD registers them."
       "Historical aliases such as xjp-deploy-center MUST NOT become active project roots."])

(project-management-taxonomy
    :schema "missiond.project-management-taxonomy.v1"
    :fields [management-domain runtime-layer deployment-channels deployment-channel-drift]
    :rule "Every registered project declares both an ownership universe and a runtime layer so MissionD does not confuse XJP platform backends, MissionD production systems, ops infrastructure, brand sites, and product-service projects."
    :management-domains
      ((domain missiond-production-system
         :meaning "MissionD, Forge, Mechanic, Jarvis, and related agent/devtool production systems.")
       (domain xiaojinpro-core-backend
         :meaning "Pure XiaojinPro universe backend programs that provide global platform support across products.")
       (domain xiaojinpro-platform-ops
         :meaning "Deployment, operational, CLI, MCP, and infrastructure control surfaces for the XJP platform.")
       (domain xiaojinpro-client-surface
         :meaning "XiaojinPro public, admin, mobile, or gateway frontend clients.")
       (domain product-service-layer
         :meaning "Standalone products and service-layer applications such as Palm Era, Search Center, ASR, WePub, CutHub, and other user-facing services.")
       (domain brand-content-site
         :meaning "Public brand, portfolio, blog, and content sites such as ruoqijin.com and jinstudio.com.")
       (domain external-infra
         :meaning "External infrastructure runtime that products consume but that is not itself a user-facing product."))
    :runtime-layers [control-plane-frontend devtool platform-monorepo support-backend support-agent ops-service ops-agent ops-tool platform-frontend mobile-client public-content-site brand-site product-fullstack product-api product-frontend external-infra-runtime]
    :invariants
      ["XiaojinPro core backends MUST NOT be used as product-service roots just because a product calls them."
       "MissionD production-system projects MUST stay separate from user-facing product-service-layer apps."
       "Brand/content sites are managed projects, but their runtime layer is not platform support backend unless they own shared APIs."
       "Project queries, Board dispatch, and deploy-ops context packs MUST preserve management-domain and runtime-layer labels."
       "MissionD project management MUST show per-project deployment-channels derived from deployment-channel-plane/service-runtime-universe: build lane, runtime target/deploy-center or VM lane, and frontend hosting lane when present."
       "Project management MUST distinguish declared MissionD channel intent, repo/workflow inferred build facts, and deploy-center live observed facts so a runtime deploy-center lane is never mistaken for a deploy-center build lane."])

(registry-authority-map
    :schema "missiond.registry-authority-map.v1"
    :authorities ((missiond :owns [project-identity ssot-paths maturity board-workstation-scheduling])
                  (deploy-center :owns [deployment-targets runtime-location release-provenance agent-executor-state])
                  (forge :owns [component-catalog pattern-catalog code-reality-mirror universe-dag-recommendations]))
    :workflow project-registry-reconciliation
    :rule "Registry reconciliation reads MissionD DB, compiled-project-universe, deploy-center, and Forge facts; reports missing_in_db, missing_in_v3, inactive_or_legacy, metadata_drift, alias_conflict, root_mismatch, and deploy_fact_missing; and never silently overwrites or deletes identities.")

(project-blueprint-registry
    :schema "missiond.project-blueprint-registry.v1"
    :rule "Project-local app blueprints are independent SSOT files registered from V3; backend V3 stays compact and aggregate checkers follow the registry pointer."
    (project :id board
      :kind frontend-nextjs
      :management-domain missiond-production-system
      :runtime-layer control-plane-frontend
      :path ".missiond/frontend/board-blueprint.lisp"
      :package "packages/board/package.json"
      :status code-aligned
      :checks ["node scripts/check-frontend-board-lisp-schema.mjs"
               "node scripts/check-frontend-board-code-isomorphism.mjs"
               "node scripts/check-frontend-board-runtime-projection.mjs"
               "node scripts/project-frontend-board-config.mjs --check"]
      :surface board-frontend)
    (project :id jarvis-forge
      :kind multi-crate-nextjs
      :management-domain missiond-production-system
      :runtime-layer devtool
      :root "/Users/jinchen/Projects/jarvis-forge"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/forge-backend-blueprint.lisp"
      :frontend ".missiond/frontend/forge-ui-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered project; Lisp/component reuse engine, not MissionD runtime orchestrator"
      :surface project-registry)
    ;; ── Part1 devtools — sibling devtool repos with M5 SSOT, registered as a group ──
    (project :id jarvis
      :kind rust-multi-crate
      :management-domain missiond-production-system
      :runtime-layer devtool
      :root "/Users/jinchen/Projects/jarvis"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/jarvis-backend-blueprint.lisp"
      :frontend ".missiond/frontend/jarvis-ui-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; clean MissionD rewrite (intent.lisp + 14 intent-*.lisp shards + GAP_ANALYSIS.md)"
      :surface project-registry)
    (project :id jarvis-mechanic
      :kind rust-cli
      :management-domain missiond-production-system
      :runtime-layer devtool
      :root "/Users/jinchen/Projects/jarvis-mechanic"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/jarvis-mechanic-backend-blueprint.lisp"
      :status incubating-project
      :checks ["node scripts/check-mechanic-ssot.mjs"
               "bash .missiond/check.sh"]
      :missiond-role "registered devtool; opt-in repair executor CLI, not a MissionD orchestrator or automatic runtime worker"
      :surface project-registry)
    (project :id xjpcode
      :kind rust-cli
      :management-domain missiond-production-system
      :runtime-layer devtool
      :root "/Users/jinchen/Projects/xjpcode"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/xjpcode-app-blueprint.lisp"
      :status project-ssot-owned
      :checks ["node scripts/check-xjpcode-ssot-complete.mjs --json"
               "node scripts/check-xjpcode-code-isomorphism.mjs"
               "node scripts/check-xjpcode-portable-worker-runtime.mjs --json"]
      :missiond-role "registered devtool and portable agent runtime candidate; read-only MissionD WorkOrder worker over /worker/v1/work-orders, with write lane gated by accepted shard + write lease"
      :surface project-registry)
    (project :id neural-codegen
      :kind rust-multi-crate
      :management-domain missiond-production-system
      :runtime-layer devtool
      :root "/Users/jinchen/Projects/neural-codegen"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/neural-codegen-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; deterministic Lisp→IR→Rust codegen pipeline"
      :surface project-registry)
    (project :id semantic-terminal
      :kind rust-napi-cdylib
      :management-domain missiond-production-system
      :runtime-layer devtool
      :root "/Users/jinchen/Projects/semantic-terminal"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/semantic-terminal-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; PTY semantic event parser (Rust core + N-API)"
      :surface project-registry)
    (project :id xiaojinpro-backend
      :kind rust-monorepo
      :management-domain xiaojinpro-core-backend
      :runtime-layer platform-monorepo
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xiaojinpro-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :checks ["node scripts/check-xjp-ssot-complete.mjs"]
      :surface project-registry)
    (project :id xjp-mcp
      :kind node-mcp-server
      :management-domain xiaojinpro-platform-ops
      :runtime-layer ops-tool
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/tools/xjp-mcp"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-mcp-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP infra tool surface; ClaudeCode/MissionD-facing MCP bridge for deploy/auth/secret/storage/router operations, not deployment fact authority"
      :surface project-registry)
    (project :id xjp-cli
      :kind rust-cli
      :management-domain xiaojinpro-platform-ops
      :runtime-layer ops-tool
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/crates/xjp-cli"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-cli-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP infra operator CLI and embedded MCP server; distinct from apps/xjp-deploy-agent remote execution daemon"
      :surface project-registry)
    (project :id deploy-center
      :aliases [xjp-deploy-center]
      :kind ops-service
      :management-domain xiaojinpro-platform-ops
      :runtime-layer ops-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-center-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :capability deploy-ops
      :note "xjp-deploy-center is a historical alias for this same canonical service root, not an active Universe project."
      :surface project-registry)
    (project :id deploy-agent
      :aliases [xjp-deploy-agent]
      :kind ops-agent
      :management-domain xiaojinpro-platform-ops
      :runtime-layer ops-agent
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps/xjp-deploy-agent"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-agent-backend-blueprint.lisp"
      :status project-ssot-owned
      :capability deploy-ops
      :surface project-registry)
    (project :id auth
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id router
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/router"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/router-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id search-center
      :aliases [xjp-search-center deep-research "聚合搜索" "搜索中心"]
      :kind rust-nextjs-service
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/search-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/search-center-backend-blueprint.lisp"
      :frontend ".missiond/frontend/search-center-web-blueprint.lisp"
      :operations ".missiond/operations/search-center-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP search product; Quick Search and Deep Research service with Rust backend, Vercel frontend, xjp-auth, router search, and deploy-center/GCP runtime boundaries"
      :surface project-registry)
    (project :id xjp-memory
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-memory"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-memory-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered memory provider service; owns private memory, review overlay, skill evidence, FTS/embedding/rerank storage behind MissionD memory-provider-contract"
      :surface project-registry)
    (project :id xjp-eventhub
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-eventhub"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-eventhub-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered EventHub service; owns cross-service durable event envelopes while MissionD local EventBus remains offline-safe"
      :surface project-registry)
    (project :id xjp-project-universe
      :aliases [project-universe "项目宇宙" "项目管理read-model" project-management-read-model]
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/project-universe"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-project-universe-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered read-only project universe service; exposes cached project list, topology, maturity, domains, deployment channels, and support catalog from MissionD compiled ABI and authority facts without parsing Lisp"
      :surface project-registry)
    (project :id xjp-image-service
      :aliases [xjp-image "图床" "图片服务" image-service]
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/image"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-image-service-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered media platform service; owns image artifacts, private signed image delivery, thumbnail/preview variants, and provider-box generated image ingestion"
      :surface project-registry)
    (project :id xjp-video-service
      :aliases [xjp-video "视频服务" video-service]
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/video"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-video-service-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered media platform service; owns video artifacts, private signed playback, poster generation, durable transcode jobs, and the internal 12900kf transcode runner protocol"
      :surface project-registry)
    (project :id xjp-domain-service
      :aliases [xjp-domain "域名服务" domain-service dns-service]
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/domain"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-domain-service-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP domain management service; owns Cloudflare and Aliyun DNS inventory, approval-gated DNS apply, durable audit, and service-domain bindings for owned XJP zones"
      :surface project-registry)
    (project :id xjp-mail-service
      :aliases [xjp-mail "邮箱服务" mail-service support-mail customer-mail google-workspace-mail]
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/mail"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-mail-service-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP support mailbox service; owns per-service logical mailboxes, Google Workspace onboarding plans, Gmail Pub/Sub sync ledger, agent draft/send approval policy, and delegates DNS mutation to xjp-domain-service"
      :surface project-registry)
    (project :id xjp-legal-service
      :aliases [xjp-legal legal-service policy-service consent-ledger "用户协议" "隐私政策"]
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/legal"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-legal-service-blueprint.lisp"
      :operations ".missiond/operations/xjp-legal-service-operations-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP legal policy service; owns immutable legal policy versions, public policy read APIs, user agreement acceptance evidence, consent grant/withdrawal ledger, and delegates legal.xiaojins.com DNS mutation to xjp-domain-service"
      :surface project-registry)
    (project :id xjp-invoice-service
      :aliases [xjp-invoice invoice-service fapiao-service "发票服务" "发票中心" "国内发票"]
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/invoice"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-invoice-service-blueprint.lisp"
      :operations ".missiond/operations/xjp-invoice-service-operations-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP invoice service; owns domestic fapiao request ledger, manual issuing records, delivery links, red-letter audit, and delegates invoice.xiaojins.com DNS mutation to xjp-domain-service while keeping order eligibility and invoice locks in payments"
      :surface project-registry)
    (project :id xjp-video-transcode-runner
      :aliases [xjp-video-runner "视频转码runner" 12900kf-video-runner]
      :kind rust-daemon
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-agent
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/video"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-video-service-blueprint.lisp"
      :status contract-first-runner
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered 12900kf Rust video transcode runner; managed by xjp-deploy-agent HostedServiceManager and communicates with xjp-video-service through the self-built proxy deployment program"
      :surface project-registry)
    (project :id payments
      :aliases ["XJP Payments" "xjp-payments" "pay.xiaojinpro.com" "微信支付" "统一收银台"]
      :kind rust-workspace-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/payments"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/payments-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id asr
      :aliases ["ASR" "XJP ASR" speech-recognition subtitle-service asr-web "语音转写" "speechscribe.top" "asr.xiaojinpro.top"]
      :kind rust-nextjs-service
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/asr"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/asr-backend-blueprint.lisp"
      :frontend ".missiond/frontend/asr-web-blueprint.lisp"
      :operations ".missiond/operations/asr-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP ASR / SpeechScribe product; independent Next.js global frontend at speechscribe.top with asr.xiaojinpro.top as compatibility alias, Rust ASR backend routed through auth.xiaojinpro.com/asr, XJP Auth + Stripe/Payments credits, and deploy-center/GCP runtime boundaries"
      :surface project-registry)
    (project :id timeline
      :kind rust-service
      :management-domain xiaojinpro-core-backend
      :runtime-layer support-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/timeline"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/timeline-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id pcea
      :kind rust-vite-app
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Downloads/PCEA develop"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/pcea-backend-blueprint.lisp"
      :frontend ".missiond/frontend/pcea-frontend-blueprint.lisp"
      :status project-ssot-owned
      :surface project-registry)
    (project :id xiaojinpro-ios
      :kind ios-swiftui-app
      :management-domain xiaojinpro-client-surface
      :runtime-layer mobile-client
      :root "/Users/jinchen/development/xiaojinproIOS/xiaojinpro"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/xiaojinpro-ios-blueprint.lisp"
      :operations ".missiond/operations/xiaojinpro-ios-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered mobile control client; iPhone entry for Jarvis/MissionD, using Auth JWT and Jarvis HTTPS proxy to control the Mac mini MissionD node"
      :surface project-registry)
    (project :id diandian
      :aliases ["等待小岛" "点点" "点点花园" "AI 等待室" "等待花园"]
      :kind ios-swiftui-app
      :management-domain product-service-layer
      :runtime-layer mobile-client
      :root "/Users/jinchen/development/diandian"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/diandian-ios-blueprint.lisp"
      :operations ".missiond/operations/diandian-operations-blueprint.lisp"
      :status incubating-project
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent iOS app; local-only SwiftUI decompression ritual for AI waiting time, with daily tap garden, local UserDefaults journal, no backend, no auth, and no network dependency in the MVP"
      :surface project-registry)
    (project :id xiaojinpro-frontend
      :aliases [xiaojinpro-web "xiaojinpro.top" "小金Pro官网" "小金Pro前端"]
      :kind nextjs-platform-frontend
      :management-domain xiaojinpro-client-surface
      :runtime-layer platform-frontend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/xiaojinpro-frontend"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/xiaojinpro-frontend-blueprint.lisp"
      :operations ".missiond/operations/xiaojinpro-frontend-operations-blueprint.lisp"
      :status incubating-project
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XiaojinPro client surface; xiaojinpro.top main platform frontend, admin console, chat entry, and multi-product hub over auth/router/deploy/storage APIs"
      :surface project-registry)
    ;; ── App + external-infra projects — already-converged with project-local check.sh runners ──
    (project :id secret-store
      :aliases [secret-store-rs]
      :kind rust-axum-microservice
      :management-domain external-infra
      :runtime-layer external-infra-runtime
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :lifecycle external-infra-runtime
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered external infra runtime; AES-256-GCM credential vault (frozen LTS) consumed by auth/deploy-center/* via xjp-config HybridSecretProvider; production endpoint ss.xiaojinpro.top is now on the GCP xjp-backend VM with Caddy proxy to the local secret-store container"
      :surface project-registry)
    (project :id xiaojin-blog
      :kind nextjs-app
      :management-domain brand-content-site
      :runtime-layer public-content-site
      :root "/Users/jinchen/Projects/xiaojin-blog"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered app; ruoqijin.com personal blog + research portal (Next.js 16 + React 19 + Drizzle/PG; standalone repo xiaojinpro-team/xiaojin-blog)"
      :surface project-registry)
    (project :id jinstudio
      :aliases ["JinStudio" "小靳后期" "jinstudio.com" jinstudio-frontend]
      :kind vite-react-brand-site
      :management-domain brand-content-site
      :runtime-layer brand-site
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/jinstudio-frontend"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/jinstudio-frontend-blueprint.lisp"
      :operations ".missiond/operations/jinstudio-operations-blueprint.lisp"
      :status incubating-project
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered brand/content site; jinstudio.com official site for JinStudio 小靳后期工作室, portfolio, service pages, notes, careers, and Supabase lead capture"
      :surface project-registry)
    (project :id cuthub
      :kind nextjs-app
      :management-domain product-service-layer
      :runtime-layer product-frontend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/cuthub-frontend"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :lifecycle canonical-temporary-downloads-checkout
      :note "Supervisor decision 39a2e6e8 — Downloads checkout accepted as temporary canonical M6 SSOT root until repo is cloned to /Users/jinchen/Projects/cuthub-frontend"
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered app; cuthub.ai frontend (Next.js 16 + React 19 + Tailwind 4 + Konva); independent repo rickyjim626/cuthub-frontend"
      :surface project-registry)
    (project :id legacy-refactor-service
      :kind node-service
      :management-domain product-service-layer
      :runtime-layer product-api
      :root "/Users/jinchen/Projects/legacy-refactor-service"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/legacy-refactor-backend-blueprint.lisp"
      :operations ".missiond/deploy/legacy-refactor-deploy-blueprint.lisp"
      :status external-product-service
      :checks ["node scripts/check-legacy-refactor-ssot.mjs --json"]
      :missiond-role "registered external product service; MissionD may orchestrate and observe jobs, while the service owns customer-safe refactor runtime and never exposes internal Lisp/IR/Forge artifacts to customers"
      :surface project-registry)
    (project :id palm-era
      :aliases [ye-dao-ji-yuan "椰岛纪元"]
      :kind rust-nextjs-game
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Projects/palm-era"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/palm-era-backend-blueprint.lisp"
      :frontend ".missiond/frontend/palm-era-frontend-blueprint.lisp"
      :operations ".missiond/operations/palm-era-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; reset tree-timer prototype with Vercel-only Next.js frontend at palm-era.xiaojinpro.top, GCP VM Rust API at palm-era-api.xiaojinpro.top, self-managed GCP VM Postgres, and current gameplay limited to planting a tree and showing elapsed time since planted_at"
      :surface project-registry)
    (project :id chat-translator
      :aliases ["chat 翻译工具" "chat翻译" chat-translation-tool]
      :kind rust-nextjs-local-agent-app
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Projects/chat-translator"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/chat-translator-backend-blueprint.lisp"
      :frontend ".missiond/frontend/chat-translator-frontend-blueprint.lisp"
      :operations ".missiond/operations/chat-translator-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; single-user chat translation and WeChat mirror tool with Rust backend, local sync agent, and Next.js frontend"
      :surface project-registry)
    (project :id long-image-service
      :aliases [changtu "长图" "长图工具" changtu-pro]
      :kind vite-express-app
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Projects/long-image-service"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/long-image-backend-blueprint.lisp"
      :frontend ".missiond/frontend/long-image-frontend-blueprint.lisp"
      :operations ".missiond/operations/long-image-deployment-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; Markdown to long-image generation service with history, membership, API rendering, and CN/global deployment boundaries"
      :surface project-registry)
    (project :id wechat-publisher
      :aliases [wepub "WePub" "微信公众号文章编辑器" wechat-article-editor]
      :kind rust-nextjs-cms
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Projects/wechat-publisher"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/wechat-publisher-backend-blueprint.lisp"
      :frontend ".missiond/frontend/wechat-publisher-frontend-blueprint.lisp"
      :operations ".missiond/operations/wechat-publisher-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; WeChat Official Account article editor with Rust backend, versioned article store, and Next.js editing workspace"
      :surface project-registry)
    (project :id problem-tutor
      :aliases ["解题辅导" problem-solver tutor-visualizer "拍照解题"]
      :kind rust-nextjs-service
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Projects/problem-tutor"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/problem-tutor-backend-blueprint.lisp"
      :frontend ".missiond/frontend/problem-tutor-frontend-blueprint.lisp"
      :operations ".missiond/operations/problem-tutor-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; image-first AI problem solving tutor at problemwise.top with strict original-problem verification, XJP Router model orchestration, Search Center recovery, generated visualization, and quote-aware follow-up Q&A"
      :surface project-registry)
    (project :id daily-spark
      :aliases ["每日一句" daily-spark-lovable]
      :kind vite-self-hosted-supabase-app
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Projects/daily-spark"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/daily-spark-backend-blueprint.lisp"
      :frontend ".missiond/frontend/daily-spark-frontend-blueprint.lisp"
      :operations ".missiond/operations/daily-spark-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; Daily Spark production runs on Vercel frontend spark.xiaojinpro.top plus GCP VM self-hosted Supabase-compatible backend/database at api.spark.xiaojinpro.top"
      :surface project-registry)
    (project :id good-things-daily
      :aliases ["开门见喜" "世界好事日报" "Good Things Daily" goodnews "goodnews.xiaojinpro.top" "goodnews-api.xiaojinpro.top"]
      :kind rust-nextjs-service
      :management-domain product-service-layer
      :runtime-layer product-fullstack
      :root "/Users/jinchen/Projects/good-things-daily"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/good-things-daily-backend-blueprint.lisp"
      :frontend ".missiond/frontend/good-things-daily-frontend-blueprint.lisp"
      :operations ".missiond/operations/good-things-daily-operations-blueprint.lisp"
      :status incubating-project
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; 开门见喜 public evidence-backed good news content feed with RSS/GDELT ingest, xjp-router claude-opus-4-6 generated titles and presentation cards, anonymous feedback, Vercel frontend at goodnews.xiaojinpro.top, privatecloud-built Rust API image deployed to GCP VM at goodnews-api.xiaojinpro.top, and xjp-pg-prod Postgres"
      :surface project-registry))
