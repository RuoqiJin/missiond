(project-registry-policy
    :desc "Lisp-owned project registry defaults for intent discovery and universe import."
    :intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
    :default-universe-manifest "$MISSIOND_PROJECTS_DIR/universe.intent.lisp"
    :env-overrides [UNIVERSE_MANIFEST]
    :invariants
      ["mission_project init/import_universe/survey MUST project intent-path candidates from project-registry-policy."
       "mission_project import_universe MUST project its default manifest from project-registry-policy; UNIVERSE_MANIFEST is only an explicit override."
       "A real MissionD project with .missiond but no project-registry-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

(project-identity-contract
    :schema "missiond.project-identity-contract.v1"
    :fields [project_id canonical_root repo_remote ssot_paths deploy_center_slug forge_project_name service_ids aliases status]
    :rule "MissionD is project identity and SSOT registry authority; deploy-center is deployment fact authority; Forge is component/pattern/reality catalog authority."
    :reconcile-action mission_project.reconcile
    :invariants
      ["MissionD Universe owns canonical project ids, roots, SSOT paths, maturity, Board links, and workstation dispatch."
       "deploy-center owns deployment targets, runtime location, release provenance, deploy agents, and executor state."
       "Forge owns component/pattern catalog, code reality mirror, and Universe DAG recommendations; Forge-only references are not deployable unless MissionD registers them."
       "Historical aliases such as xjp-deploy-center MUST NOT become active project roots."])

(registry-authority-map
    :schema "missiond.registry-authority-map.v1"
    :authorities ((missiond :owns [project-identity ssot-paths maturity board-workstation-scheduling])
                  (deploy-center :owns [deployment-targets runtime-location release-provenance agent-executor-state])
                  (forge :owns [component-catalog pattern-catalog code-reality-mirror universe-dag-recommendations]))
    :workflow project-registry-reconciliation
    :rule "Registry reconciliation reads MissionD, deploy-center, and Forge facts, reports missing_in_*, alias_conflict, root_mismatch, and deploy_fact_missing, and never silently overwrites identities.")

(project-blueprint-registry
    :schema "missiond.project-blueprint-registry.v1"
    :rule "Project-local app blueprints are independent SSOT files registered from V3; backend V3 stays compact and aggregate checkers follow the registry pointer."
    (project :id board
      :kind frontend-nextjs
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
      :root "/Users/jinchen/Projects/neural-codegen"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/neural-codegen-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; deterministic Lisp→IR→Rust codegen pipeline"
      :surface project-registry)
    (project :id semantic-terminal
      :kind rust-napi-cdylib
      :root "/Users/jinchen/Projects/semantic-terminal"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/semantic-terminal-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; PTY semantic event parser (Rust core + N-API)"
      :surface project-registry)
    (project :id xiaojinpro-backend
      :kind rust-monorepo
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xiaojinpro-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :checks ["node scripts/check-xjp-ssot-complete.mjs"]
      :surface project-registry)
    (project :id xjp-mcp
      :kind node-mcp-server
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/tools/xjp-mcp"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-mcp-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP infra tool surface; ClaudeCode/MissionD-facing MCP bridge for deploy/auth/secret/storage/router operations, not deployment fact authority"
      :surface project-registry)
    (project :id xjp-cli
      :kind rust-cli
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
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps/xjp-deploy-agent"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-agent-backend-blueprint.lisp"
      :status project-ssot-owned
      :capability deploy-ops
      :surface project-registry)
    (project :id auth
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id router
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/router"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/router-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id search-center
      :aliases [xjp-search-center deep-research "聚合搜索" "搜索中心"]
      :kind rust-nextjs-service
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
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-memory"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-memory-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered memory provider service; owns private memory, review overlay, skill evidence, FTS/embedding/rerank storage behind MissionD memory-provider-contract"
      :surface project-registry)
    (project :id xjp-eventhub
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-eventhub"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-eventhub-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered EventHub service; owns cross-service durable event envelopes while MissionD local EventBus remains offline-safe"
      :surface project-registry)
    (project :id payments
      :kind rust-workspace-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/payments"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/payments-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id asr
      :aliases ["ASR" speech-recognition subtitle-service]
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/asr"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/asr-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id timeline
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/timeline"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/timeline-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id pcea
      :kind rust-vite-app
      :root "/Users/jinchen/Downloads/PCEA develop"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/pcea-backend-blueprint.lisp"
      :frontend ".missiond/frontend/pcea-frontend-blueprint.lisp"
      :status project-ssot-owned
      :surface project-registry)
    (project :id xiaojinpro-ios
      :kind ios-swiftui-app
      :root "/Users/jinchen/development/xiaojinproIOS/xiaojinpro"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/xiaojinpro-ios-blueprint.lisp"
      :operations ".missiond/operations/xiaojinpro-ios-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered mobile control client; iPhone entry for Jarvis/MissionD, using Auth JWT and Jarvis HTTPS proxy to control the Mac mini MissionD node"
      :surface project-registry)
    ;; ── App + external-infra projects — already-converged with project-local check.sh runners ──
    (project :id secret-store
      :aliases [secret-store-rs]
      :kind rust-axum-microservice
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :lifecycle external-infra-runtime
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered external infra runtime; AES-256-GCM credential vault (frozen LTS) consumed by auth/deploy-center/* via xjp-config HybridSecretProvider; production endpoint ss.xiaojinpro.top is now on the GCP xjp-backend VM with Caddy proxy to the local secret-store container"
      :surface project-registry)
    (project :id xiaojin-blog
      :kind nextjs-app
      :root "/Users/jinchen/Projects/xiaojin-blog"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered app; ruoqijin.com personal blog + research portal (Next.js 16 + React 19 + Drizzle/PG; standalone repo xiaojinpro-team/xiaojin-blog)"
      :surface project-registry)
    (project :id cuthub
      :kind nextjs-app
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
      :root "/Users/jinchen/Projects/palm-era"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/palm-era-backend-blueprint.lisp"
      :frontend ".missiond/frontend/palm-era-frontend-blueprint.lisp"
      :operations ".missiond/operations/palm-era-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; text island industrial management simulator with Vercel-only Next.js frontend at palm-era.xiaojinpro.top, GCP VM Rust authoritative simulation backend at palm-era-api.xiaojinpro.top, and self-managed GCP VM Postgres"
      :surface project-registry)
    (project :id chat-translator
      :aliases ["chat 翻译工具" "chat翻译" chat-translation-tool]
      :kind rust-nextjs-local-agent-app
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
      :root "/Users/jinchen/Projects/wechat-publisher"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/wechat-publisher-backend-blueprint.lisp"
      :frontend ".missiond/frontend/wechat-publisher-frontend-blueprint.lisp"
      :operations ".missiond/operations/wechat-publisher-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app; WeChat Official Account article editor with Rust backend, versioned article store, and Next.js editing workspace"
      :surface project-registry)
    (project :id daily-spark
      :aliases ["每日一句" daily-spark-lovable]
      :kind vite-self-hosted-supabase-app
      :root "/Users/jinchen/Projects/daily-spark"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/daily-spark-backend-blueprint.lisp"
      :frontend ".missiond/frontend/daily-spark-frontend-blueprint.lisp"
      :operations ".missiond/operations/daily-spark-operations-blueprint.lisp"
      :status incubating-project
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered independent app migration; Daily Spark moves Lovable/Supabase Cloud runtime to Vercel frontend spark.xiaojinpro.top plus GCP VM self-hosted Supabase-compatible backend/database"
      :surface project-registry))
