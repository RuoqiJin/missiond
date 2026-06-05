(service-layer-template
    :schema "missiond.service-layer-template.v1"
    :id product-service-layer-standard
    :operator-guide "docs/guides/product-service-layer-template.md"
    :applies-to [product-service-layer]
    :runtime-layers [product-fullstack product-api product-frontend]
    :examples
      ((example :project palm-era :aliases ["椰岛纪元"] :shape product-fullstack)
       (example :project search-center :aliases ["聚合搜索" "搜索中心"] :shape product-fullstack)
       (example :project wechat-publisher :aliases [wepub "公众号发布"] :shape product-fullstack)
       (example :project asr :aliases [ASR "语音识别"] :shape product-fullstack)
       (example :project long-image-service :aliases ["长图"] :shape product-fullstack)
       (example :project chat-translator :aliases ["chat翻译"] :shape product-fullstack)
       (example :project cuthub :aliases [CutHub] :shape product-frontend))
    :rule "A new user-facing product or service-layer project MUST start from this template before code generation, deploy setup, or MissionD registration. The template standardizes repo placement, stack selection, auth, payment, database, secret-store, privatecloud Rust build lane, Vercel frontend deployment, login protection, local SSOT, and project-universe registration."
    :agent-triggers
      ["用户说要新起一个服务、产品、工具、站点、AI app、编辑器、导出器、聚合工具、ASR/长图/chat翻译类服务时"
       "project management taxonomy chooses management-domain=product-service-layer"
       "MissionD registers an incubating project below M5 that still needs a standard framework"]

    (decision-matrix
      :schema "missiond.service-layer-decision-matrix.v1"
      (decision :id repo-placement
        :default independent-repo
        :options
          ((option independent-repo
             :root-pattern "/Users/jinchen/Projects/<project-id>"
             :use-when "The service is a standalone product with its own frontend/backend lifecycle, public domain, or product-specific data model.")
           (option xjp-monorepo-service
             :root-pattern "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/<service-id>"
             :use-when "The service is tightly coupled to XJP core deployment, shared Rust crates, deploy-center container runtime, or internal platform backends.")
           (option frontend-only
             :root-pattern "/Users/jinchen/Projects/<project-id> or existing frontend repo"
             :use-when "The product has no durable backend and only calls existing XJP APIs.")))
      (decision :id frontend-stack
        :default nextjs-react-typescript-tailwind
        :options
          ((option nextjs-react-typescript-tailwind
             :stack "Next.js 16 + React 19 + TypeScript + Tailwind 4"
             :use-when "Default for new service-layer products, auth-protected surfaces, API client SDKs, SEO pages, and Vercel deployment.")
           (option vite-react-typescript
             :stack "Vite + React + TypeScript"
             :use-when "Existing Vite project, static brand/editor surface, or browser-heavy export tool that does not need App Router/server components.")
           (option no-frontend
             :stack "Backend API only"
             :use-when "Machine API, worker, ingest service, or local-agent bridge.")))
      (decision :id backend-stack
        :default rust-axum-sqlx-postgres
        :options
          ((option rust-axum-sqlx-postgres
             :stack "Rust axum + sqlx + PostgreSQL"
             :use-when "Default for durable data, auth/payment-sensitive flows, event logs, workers, API products, or M6 target services.")
           (option nextjs-route-handlers
             :stack "Next.js route handlers"
             :use-when "Thin CRUD/BFF APIs with no reusable domain core, strict worker model, or payment ledger.")
           (option local-agent-plus-cloud-api
             :stack "Local agent + cloud API"
             :use-when "Source data lives on a Mac, local filesystem, WeChat desktop, LAN, or private device state.")))
      (decision :id database
        :default supabase-postgres-session-pooler
        :options
          ((option supabase-postgres-session-pooler
             :use-when "New independent service needs durable PostgreSQL on Vercel."
             :connection "Use Supabase pooler session mode on port 5432 with sslmode=require; never guess the pooler host.")
           (option xjp-postgres
             :use-when "Monorepo services deployed through deploy-center consume existing XJP runtime PostgreSQL.")
           (option no-db
             :use-when "Frontend-only or stateless proxy service.")))
      (decision :id rust-build-lane
        :default privatecloud-rust-build
        :options
          ((option privatecloud-rust-build
             :builder privatecloud-10900kf
             :parallel_linux_build_pool [privatecloud-10900kf windows-12900kf-linux-vm]
             :runner_labels [privatecloud-agent]
             :runner_agent_id none
             :authority deploy-center
             :use-when "Default for every Rust product-service backend build, including services that run later on GCP VM, ECS, or another container host. deploy-center native_workflow should use runner_labels plus required_capabilities so 10900KF and 12900KF Linux VM runners can claim jobs in parallel; runner_agent_id is reserved for explicit single-host affinity with runner_pin_rationale.")
           (option nextjs-route-handler-no-rust-build
             :use-when "Thin Next.js BFF with no Rust backend.")
           (option explicit-vercel-rust-exception
             :use-when "Only with written MissionD/deploy-center exception evidence; not the scaffold default."))))

    (repo-layout
      :schema "missiond.service-layer-repo-layout.v1"
      :independent
        ((path "frontend/" :owns [nextjs-app-router ui api-client auth-client proxy])
         (path "backend/" :owns [rust-axum-domain api auth db migrations jobs])
         (path "backend/api/axum.rs" :owns [explicit-exception-vercel-rust-function-entry] :optional true)
         (path "backend/src/lib.rs" :owns [build_app connect_db app_state])
         (path "backend/src/main.rs" :owns [local-dev-server])
         (path "backend/src/auth.rs" :owns [jwks-verification service-token-guard])
         (path "backend/src/db.rs" :owns [sqlx-pool queries])
         (path "backend/migrations/" :owns [idempotent-sql])
         (path ".missiond/" :owns [intent backend-blueprint frontend-blueprint operations-blueprint final-report checker behavior-universe])
         (path "vercel.json" :owns [frontend-route-map frontend-env domain-aliases]))
      :monorepo-service
        ((path "services/<service-id>/" :owns [rust-service domain api migrations checkers])
         (path "apps/<service-id>-web/" :owns [nextjs-frontend] :optional true)
         (path "services/<service-id>/.missiond/" :owns [project-local-ssot])
         (path "infra/caddy/Caddyfile" :owns [api-proxy] :optional true)
         (path "services.yaml" :owns [deploy-center-service-entry] :optional true)))

    (auth-standard
      :schema "missiond.service-layer-auth-standard.v1"
      :default-provider xjp-auth
      :public-client-flow pkce
      :browser-oauth-flow
        (:authorize-endpoint "https://auth.xiaojinpro.com/oauth2/authorize"
         :token-endpoint "https://auth.xiaojinpro.com/oauth2/token"
         :response-type code
         :code-challenge-method S256
         :default-scopes [openid profile email offline_access]
         :forbidden ["legacy /oauth/authorize" "implicit response_type=token"])
      :machine-flow service-api-key
      :oauth-client-creation
        (:authority auth-admin-api
         :rule "Create or update OAuth clients through xjp_oauth_client_create or the auth Admin API/MCP surface; do not edit oauth_clients rows directly."
         :default-scopes [openid profile email offline_access]
         :redirect-uris ["https://<canonical-domain>/auth/callback" "https://www.<canonical-domain>/auth/callback" "https://<compat-domain>/auth/callback" "http://localhost:<port>/auth/callback"]
         :allowlist-rule "Every production, www, compatibility, and Vercel/custom domain that can initiate login MUST be present in the OAuth client redirect allowlist before or with domain cutover; wildcards are forbidden."
         :update-rule "Existing services changing domains must call xjp_oauth_client_update or the auth Admin API/MCP surface before deploy verification; auth rebuild/restart is not required for redirect URI changes."
         :redirect-smoke
           ((step s1 :id compare-runtime-domains :logic "Compare service-runtime public_base_url, frontend_url, api_base_url, domains, and Vercel production domains with OAuth redirect_uris.")
            (step s2 :id google-authorize-smoke :logic "Click Google login or call authorize from each live host with response_type=code and PKCE; fail the release on invalid_client / unsupported_response_type / invalid_request / Invalid redirect_uri.")
            (step s3 :id callback-roundtrip :logic "Verify /auth/callback returns to the initiating host, stores marker/session state, and reaches a protected route without redirect loops.")))
      :frontend-login-protection
        ((step s1 :id public-route-map :logic "declare public routes: /, /auth/login, /auth/callback, health, public assets, and product-specific public pages")
         (step s2 :id marker-cookie-gate :logic "Next.js proxy.ts protects private route prefixes using a short-lived marker cookie; unauthenticated users redirect to /auth/login")
         (step s3 :id token-storage :logic "browser stores access token only according to the project auth policy; refresh token handling must be explicit and regression-tested")
         (step s4 :id api-client-auth :logic "frontend API client attaches bearer tokens only to same-service API calls and handles 401 refresh/logout deterministically"))
      :backend-verification
        ((step s1 :id jwks-issuer :logic "backend verifies iss/aud/exp/signature against XJP Auth issuer and JWKS URL")
         (step s2 :id user-context :logic "handlers receive tenant_id, user_id, scopes, product entitlements, and privacy class from an auth extractor")
         (step s3 :id service-token :logic "machine endpoints use service API keys or ingest tokens; never reuse browser OAuth tokens for background workers"))
      :env [XJP_AUTH_ISSUER XJP_AUTH_JWKS_URL XJP_AUTH_CLIENT_ID XJP_AUTH_AUDIENCE])

    (payment-standard
      :schema "missiond.service-layer-payment-standard.v1"
      :default-provider xjp-payments
      :rule "Product services consume the shared payments service for product, price, order, subscription, entitlement, and webhook truth. Frontends display payment state but do not own payment truth."
      :required-contracts [product-code price-plan entitlement-policy webhook-verification refund-policy billing-region]
      :backend-steps
        ((step s1 :id define-product :logic "declare product_code, plan ids, feature gates, and entitlement mapping in project SSOT before adding UI")
         (step s2 :id verify-entitlement :logic "backend checks entitlement through payments/auth claims before premium operations")
         (step s3 :id webhook-ingest :logic "payment webhooks verify signatures, persist idempotently, and emit service events")
         (step s4 :id no-local-ledger-authority :logic "local project tables may cache payment state but payments service remains authority"))
      :env [PAYMENTS_API_BASE_URL PAYMENTS_SERVICE_TOKEN PAYMENTS_WEBHOOK_SECRET])

    (support-mail-standard
      :schema "missiond.service-layer-support-mail-standard.v1"
      :default-provider xjp-mail-service
      :mailbox-model hybrid
      :rule "Every public product service declares a support mailbox plan before production promotion. MissionD logical mailbox ownership is per service; physical Google Workspace mailbox may be dedicated_user or alias."
      :required-contracts [support-address mailbox-kind target-user domain-readiness dns-requirements agent-policy]
      :provider
        (:initial google-workspace
         :domain-service xjp-domain-service
         :mail-service-api "https://mail.xiaojins.com/v1/mail"
         :dns-records [MX TXT]
         :required-dns [gmail_mx spf dmarc_monitoring]
         :dkim "generated in Google Admin after Gmail activation; store requirement/manual step, not private key material")
      :agent-policy
        (:default-mode draft-only
         :default-actions [read draft_reply]
         :auto-send "disabled unless mail_agent_policies explicitly enables it for the service"
         :audit-table mail_audits)
      :provisioning-steps
        ((step s1 :id choose-support-address :logic "Choose support/help/legal/privacy addresses and decide dedicated_user vs alias using traffic, brand, and access-boundary needs.")
         (step s2 :id plan-mailbox :logic "Call xjp-mail-service POST /v1/mail/services/:service_id/mailboxes/plan; persist DNS requirements and mailbox ledger.")
         (step s3 :id plan-domain-dns :logic "Use xjp-domain-service /records/plan for MX/SPF/DMARC; direct Cloudflare mutation is break-glass only.")
         (step s4 :id apply-approved-mailbox :logic "Apply mailbox only with x-mail-approval-token; Google Workspace provider actions remain explicit and audited.")
         (step s5 :id verify-readiness :logic "Check /v1/mail/domains/:domain/readiness and /v1/domains/services/:service_id/readiness before production promotion."))
      :env [MAIL_API_BASE_URL MAIL_SERVICE_TOKEN SUPPORT_MAILBOX_ADDRESS SUPPORT_MAILBOX_KIND SUPPORT_MAILBOX_TARGET_USER])

    (database-standard
      :schema "missiond.service-layer-database-standard.v1"
      :default supabase-postgres
      :rules
        ["Use Supabase session pooler port 5432 for Vercel + sqlx; avoid transaction pooler unless the driver is proven pgbouncer-safe."
         "Do not run production migrations during Vercel cold start."
         "Migrations must be idempotent: CREATE TABLE IF NOT EXISTS and ALTER TABLE ADD COLUMN IF NOT EXISTS."
         "Tables for shared Supabase projects MUST be namespaced by project concept."
         "Data-bearing services MUST declare region, data classes, cross-region default, and retention policy in MissionD SSOT."
         "SPI, payment ledger, private user content, generated media, and WeChat/local-device data require explicit privacy class and export/delete policy."]
      :rust-pool
        (:max-connections 4
         :acquire-timeout-secs 10
         :statement-cache-capacity 0)
      :env [DATABASE_URL])

    (rust-build-lane-standard
      :schema "missiond.service-layer-rust-build-lane.v1"
      :default privatecloud-rust-build
      :builder_pool [privatecloud-10900kf windows-12900kf-linux-vm]
      :runner_labels [privatecloud-agent]
      :runner_agent_id none
      :authority deploy-center
      :deployment-channel-summary [build-lane runtime-target frontend-hosting deployment-channel-plane]
      :rule "Rust product-service backend builds MUST run through deploy-center approved privatecloud/codebase build lane with deploy_project_stage_configs.build.config.deploy_type=native_workflow, runner_labels/required_capabilities for the generic 10900KF plus 12900KF Linux VM build pool, and no native_workflow.runner_agent_id unless an explicit runner_pin_rationale documents temporary single-host affinity. Vercel and production runtime targets such as gcp-runtime/GCP VM are deploy targets, not Rust builders; docker_build plus source_strategy=xjp_native_codebase_runner is migration compatibility only."
      :pipeline
        ((step s1 :id source-sync :logic "Sync the release commit through deploy-center/codebase source synchronization; GitHub Actions may be a control-plane trigger only.")
         (step s2 :id native-stage-dispatch :logic "Normal deploy-center trigger dispatch creates xjp_workflow_runs/xjp_workflow_jobs for the build stage when deploy_type=native_workflow.")
         (step s3 :id privatecloud-build :logic "Run cargo build/docker build on the deploy-center approved privatecloud/codebase builder with cache, registry login, and secret refs supplied by the build lane.")
         (step s4 :id publish-artifact :logic "Publish image or binary artifact with source commit, builder id, image digest/artifact sha256, and rollback reference.")
         (step s5 :id runtime-deploy :logic "Runtime targets such as GCP VM pull/recreate from the built artifact and run health smoke; they must not compile or build images in production.")
         (step s6 :id provenance :logic "Close deploy-center release provenance before MissionD maturity/deploy checks accept the rollout."))
      :forbidden
        ["Do not run cargo build, docker build, or docker compose up --build on a production GCP VM/runtime target for a product-service Rust backend."
         "Do not use Vercel Rust Function as the default product-service Rust backend deployment lane."
         "Do not use an operator laptop build as release evidence except audited break-glass bootstrap with follow-up lane repair."]
      :evidence [source_commit builder_id workflow_run_id artifact_digest runtime_target smoke_result deploy_center_provenance])

    (secret-store-standard
      :schema "missiond.service-layer-secret-standard.v1"
      :authority secret-store
      :secret-path-pattern "projects/<project-id>/<environment>/<SECRET_NAME>"
      :rules
        ["Never commit secret values to Lisp, Markdown, .env examples, Vercel config, or code."
         "MissionD SSOT may name env vars, secret refs, owners, and required capabilities only."
         "Production deploys must fail closed when a required secret ref is missing."
         "Provider keys, payment tokens, webhook secrets, database passwords, and service API keys live in secret-store or the approved deploy environment injection surface."
         "Vercel env vars are projections from approved secret refs or explicitly approved environment values, not independent authority."]
      :required-classes [database auth-client payment provider webhook object-storage service-token]
      :env [SECRET_STORE_URL SECRET_STORE_TOKEN SECRET_STORE_PROJECT_PREFIX])

    (xjp-auth-token-governance
      :schema "missiond.xjp-auth-token-governance.v1"
      :authority auth
      :secret-material-authority secret-store
      :issuer "https://auth.xiaojinpro.com"
      :jwks-url "https://auth.xiaojinpro.com/.well-known/jwks.json"
      :token-endpoint "https://auth.xiaojinpro.com/oauth2/token"
      :service-token-flow client_credentials
      :required-audience true
      :service-accounts [missiond jarvis router deploy-center xjp-image-service xjp-video-service xjp-code-center secret-store]
      :scopes [service:missiond service:jarvis service:router service:deploy-center service:xjp-image-service service:xjp-video-service service:xjp-code-center secret:read secret:write media:import media:transcode router:invoke deploy:execute missiond:interact]
      :secret-ref-policy
        (:production strict
         :compat-fallback "allowed only when explicitly not production and visible in monitor/checker"
         :missing-ref "typed dependency error, not env guessing"
         :break-glass "Secret Store local API key is disabled unless SECRET_STORE_BREAKGLASS_ENABLED=1 and audit is mandatory")
      :runtime-projection
        (:auth [issuer jwks_url token_endpoint allowed_audiences service_accounts scopes]
         :secret-store [strict legacy_fallback_status break_glass_status auth_jwt_required]
         :service-token [audience cache_until_expiry refresh_before_expiry_seconds]
         :monitor [auth_issuer auth_jwks service_token_mode secret_store_strictness legacy_fallback_status media_upload_readiness])
      :rules
        ["Auth owns identity, service accounts, token issuance, scopes/capabilities, and audit."
         "Secret Store stores encrypted secret material, versions, rotation metadata, and namespace ACLs; it must authorize normal production reads with Auth-issued JWT subjects and scopes."
         "Production services must not rely on undeclared env fallbacks for provider keys, GitHub/Vercel/Object Storage/webhook/DNS credentials, media import tokens, Router provider keys, or MissionD interaction tokens."
         "Migration fallback is allowed only when strict mode is disabled and monitor/checker reports the fallback state."
         "Secret values never appear in Lisp, runtime artifacts, monitor diagnostics, SSE, Jarvis context, grounding reports, or artifact previews; only secret refs, credential type, target id, and redaction fingerprint may be shown."]
      :interfaces
        ((auth-token :method POST :path "/oauth2/token" :grant_type client_credentials :requires [client_id client_secret audience] :returns "short-lived bearer JWT")
         (secret-store-auth :header "Authorization: Bearer <auth-jwt>" :scope [secret:read secret:write secret:*])
         (media-import :header "Authorization: Bearer <auth-jwt>" :scope [media:import media:transcode])
         (jarvis-monitor :path "/api/monitor/jarvis" :schema "missiond.jarvis-chain-monitor.v2" :adds [auth_secret_readiness jarvis_auth_secret_readiness]))
      :env [AUTH_ISSUER AUTH_JWKS_URL AUTH_TOKEN_ENDPOINT SECRET_STORE_STRICT SECRET_STORE_ALLOW_ENV_FALLBACK SECRET_STORE_BREAKGLASS_ENABLED])

    (vercel-standard
      :schema "missiond.service-layer-vercel-standard.v1"
      :default-target vercel-frontend
      :frontend-route "/"
      :backend-route-prefix "/api"
      :root-vercel-json
        (:pattern "frontend-only default; experimentalServices is legacy or explicit exception only"
         :frontend-entrypoint "frontend"
         :frontend-framework nextjs
         :backend-entrypoint "backend/api/axum.rs"
         :backend-framework rust-axum
         :rule "Vercel deploys the frontend by default. Rust backend deployment uses rust-build-lane-standard; backend/api/axum.rs and experimentalServices require explicit MissionD/deploy-center exception evidence. If an exception uses routePrefix /api, axum routes must include /api because Vercel does not strip routePrefix.")
      :deployment-rules
        ["Frontend production domains are declared in service-runtime-universe."
         "Vercel project, env vars, and domains must be recorded as deployment facts or risks."
         "Project management must be able to show the frontend hosting channel next to the Rust build lane and runtime target channel."
         "Use deploy-center provenance when backend runs outside Vercel."
         "GCP VM backend deploy stages pull already-built privatecloud artifacts; they do not run docker compose up --build or cargo build."
         "Run browser auth smoke for login-protected services before promoting maturity."]
      :env [NEXT_PUBLIC_APP_URL NEXT_PUBLIC_API_BASE_URL VERCEL_PROJECT_ID VERCEL_ORG_ID])

    (project-local-ssot-scaffold
      :schema "missiond.service-layer-local-ssot-scaffold.v1"
      :required-files
        [".missiond/intent.lisp"
         ".missiond/backend/<project-id>-backend-blueprint.lisp"
         ".missiond/frontend/<project-id>-frontend-blueprint.lisp"
         ".missiond/operations/<project-id>-operations-blueprint.lisp"
         ".missiond/evidence/<project-id>-final-m6-report.lisp"
         ".missiond/behavior-universe.lisp"
         ".missiond/check.sh"]
      :intent-must-declare [purpose aliases owner management-domain runtime-layer data-classes auth db deployment domains risks]
      :backend-blueprint-must-declare [domain-model api-routes auth-extractor db-boundary payment-boundary event-log worker-jobs runtime-projection]
      :frontend-blueprint-must-declare [routes public-routes protected-routes auth-callback api-client error-states loading-states regression-smokes]
      :operations-blueprint-must-declare [vercel deploy-center privatecloud-rust-build-lane secret-store supabase migrations oauth-redirect-allowlist auth-smoke health-smoke rollback-risks]
      :service-runtime-must-project [buildLane deployment frontendDeployment deploymentChannels]
      :deployment-channel-plane-must-declare [build native_workflow privatecloud-builder runtime-target frontend-hosting drift-status-source]
      :checker-must-run [package-manager-check backend-build frontend-build behavior-closure ssot-shape secret-value-redaction])

    (deployment-closure-bundle-standard
      :schema "missiond.service-layer-deployment-closure-bundle.v1"
      :generator "node scripts/scaffold-product-deployment-closure.mjs --project-id <project-id> --name <name> --domain <frontend-domain> --api-domain <api-domain>"
      :required-files
        ["service.manifest.toml"
         "deploy/deploy-center/project.json"
         "deploy/deployment-closure/preflight.json"
         "deploy/deployment-closure/runtime-target.json"
         "deploy/deployment-closure/db-adoption-plan.json"
         "deploy/deployment-closure/domain-plan.json"
         "deploy/deployment-closure/rollback-plan.json"
         "deploy/vercel/project.json"
         "deploy/gcp-vm/compose.yaml"
         "deploy/gcp-vm/.env.example"
         "vercel.json"
         ".missiond/operations/<project-id>-operations-blueprint.lisp"
         ".missiond/check.sh"]
      :manifest-must-declare [deploy_project healthcheck.deep env.required smoke deps]
      :deploy-center-project-must-declare [manifest_required immutable_image_required runtime_digest_required smoke_required db_adoption_required release_lease_required artifact_lane target_side_build_allowed_false diagnostic_profiles build_stage_native_workflow]
      :runtime-target-must-declare [runtime_target compose_files image_env required_running_digest target_side_build_allowed_false]
      :preflight-must-declare [DeploymentIntent ReleaseCandidate ReleaseLease RuntimeObservation ReleaseEvidence ClosureVerdict fail_closed_if]
      :db-adoption-must-declare [migration_directory production_migrations_not_startup state_required_for_closure]
      :domain-plan-must-declare [xjp-domain-service authority no-direct-cloudflare frontend-domain api-domain support-mailbox]
      :rollback-plan-must-declare [previous_image_digest compose_files approval_required post_rollback_evidence]
      :rule "New product-service deploy scaffolds MUST materialize a deployment closure bundle before production deploy. Missing service.manifest.toml, deploy-center project slug, native_workflow build stage, runtime target, Secret Store refs, DB adoption plan, domain plan, or rollback artifact is a fail-closed blocker. The generated compose runtime must consume an immutable image digest and must not contain a build section.")

    (missiond-registration-scaffold
      :schema "missiond.service-layer-registration-scaffold.v1"
      :central-files
        [".missiond/v3/shards/universe/project-registry.lisp"
         ".missiond/v3/shards/universe/project-maturity.lisp"
         ".missiond/v3/shards/universe/service-runtime.lisp"
         "scripts/check-project-ssot-universe.mjs"]
      :default-registry
        (:management-domain product-service-layer
         :runtime-layer product-fullstack
         :status incubating-project
         :checks ["bash .missiond/check.sh"])
      :default-maturity (:current M2 :target M6 :gap [domain-model auth-flow-regressions payment-entitlement-regressions production-deploy-provenance final-m6-report])
      :rule "New service-layer projects start visible as incubating M2 unless their project-local SSOT and checkers prove higher maturity.")

    (agent-bootstrap-flow
      :schema "missiond.service-layer-agent-bootstrap-flow.v1"
      :entry [new-service-request product-service-layer-template]
      :core
        ((step s1 :id classify-product-service :logic "Confirm the request is product-service-layer, not xiaojinpro-core-backend, missiond-production-system, brand-content-site, or external-infra.")
         (step s2 :id collect-first-pass-facts :logic "Collect project id, human name, aliases, repo root, domain, Vercel project, auth choice, DB choice, payment need, storage need, data classes, deployment target, and region.")
         (step s3 :id choose-archetype :logic "Select product-fullstack, product-api, product-frontend, or local-agent-plus-cloud-api using the decision matrix.")
         (step s4 :id scaffold-repo :logic "Create or update repo layout, package files, backend layout, frontend layout, migrations, env examples without secret values, and root vercel.json when applicable.")
         (step s5 :id write-project-ssot :logic "Write project-local .missiond intent/backend/frontend/operations/evidence/behavior/check files from the scaffold contract.")
         (step s6 :id configure-auth :logic "Create XJP Auth client via Admin API/MCP, set redirect URIs, implement frontend callback/proxy and backend JWKS verification.")
         (step s7 :id configure-support-mail :logic "Plan support mailbox through xjp-mail-service, route DNS requirements through xjp-domain-service, and record readiness checks in operations SSOT.")
         (step s8 :id configure-data-payment-secrets :logic "Provision DB/migrations, payment product/entitlements when needed, and secret-store refs/env names without writing secret values.")
         (step s9 :id configure-deploy :logic "Generate the deployment-closure bundle with scripts/scaffold-product-deployment-closure.mjs, then configure Vercel frontend deployment plus deploy-center/privatecloud Rust build lane, runtime target deploy, domains, health checks, release lease, runtime evidence, rollback artifact, and ClosureVerdict; production migrations remain explicit operations.")
         (step s10 :id register-missiond-universe :logic "Add central project-registry, maturity, service-runtime, and checker entries with management-domain=product-service-layer.")
         (step s11 :id verify :logic "Run project checker, behavior closure, MissionD project universe, runtime compile, contract generation, auth redirect smoke, domain readiness, support mailbox readiness, and maturity gate. Report remaining gaps instead of promoting maturity silently."))
      :egress [repo-scaffold project-local-ssot missiond-registration auth-client db-migrations secret-refs vercel-config privatecloud-build-lane deployment-closure-bundle verification-report])

    :forbidden-shortcuts
      ["Do not classify product services as xiaojinpro-core-backend just because they call auth/router/payments."
       "Do not put user-facing service code inside MissionD production-system repos."
       "Do not write secret values into generated files."
       "Do not bypass XJP Auth with ad-hoc local JWT parsing for protected products."
       "Do not make payment UI before product_code, entitlement, and webhook verification are declared."
       "Do not create support mailboxes by directly mutating Google Admin or Cloudflare outside xjp-mail-service and xjp-domain-service, except through an audited break-glass path."
       "Do not run production migrations automatically during cold start."
       "Do not build Rust product-service backends on production GCP VM/runtime targets; use deploy-center privatecloud/codebase build lane and deploy only built artifacts."
       "Do not mark a new service M5/M6 without local SSOT, behavior closure, deployment provenance, and regression evidence."]
    :checker "node scripts/check-project-ssot-universe.mjs --engine=ocaml")
