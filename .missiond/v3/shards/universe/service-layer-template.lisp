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
    :rule "A new user-facing product or service-layer project MUST start from this template before code generation, deploy setup, or MissionD registration. The template standardizes repo placement, stack selection, auth, payment, database, secret-store, Vercel, login protection, local SSOT, and project-universe registration."
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
             :use-when "Frontend-only or stateless proxy service."))))

    (repo-layout
      :schema "missiond.service-layer-repo-layout.v1"
      :independent
        ((path "frontend/" :owns [nextjs-app-router ui api-client auth-client proxy])
         (path "backend/" :owns [rust-axum-domain api auth db migrations jobs])
         (path "backend/api/axum.rs" :owns [vercel-rust-function-entry])
         (path "backend/src/lib.rs" :owns [build_app connect_db app_state])
         (path "backend/src/main.rs" :owns [local-dev-server])
         (path "backend/src/auth.rs" :owns [jwks-verification service-token-guard])
         (path "backend/src/db.rs" :owns [sqlx-pool queries])
         (path "backend/migrations/" :owns [idempotent-sql])
         (path ".missiond/" :owns [intent backend-blueprint frontend-blueprint operations-blueprint final-report checker behavior-universe])
         (path "vercel.json" :owns [frontend-backend-route-map]))
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
      :machine-flow service-api-key
      :oauth-client-creation
        (:authority auth-admin-api
         :rule "Create or update OAuth clients through xjp_oauth_client_create or the auth Admin API/MCP surface; do not edit oauth_clients rows directly."
         :default-scopes [openid profile email offline_access]
         :redirect-uris ["https://<domain>/auth/callback" "http://localhost:<port>/auth/callback"])
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

    (vercel-standard
      :schema "missiond.service-layer-vercel-standard.v1"
      :default-target vercel-services
      :frontend-route "/"
      :backend-route-prefix "/api"
      :root-vercel-json
        (:pattern "experimentalServices"
         :frontend-entrypoint "frontend"
         :frontend-framework nextjs
         :backend-entrypoint "backend/api/axum.rs"
         :backend-framework rust-axum
         :rule "Vercel does not strip routePrefix; axum routes must include /api when routePrefix is /api.")
      :deployment-rules
        ["Frontend production domains are declared in service-runtime-universe."
         "Vercel project, env vars, and domains must be recorded as deployment facts or risks."
         "Use deploy-center provenance when backend runs outside Vercel."
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
      :operations-blueprint-must-declare [vercel deploy-center secret-store supabase migrations health-smoke rollback-risks]
      :checker-must-run [package-manager-check backend-build frontend-build behavior-closure ssot-shape secret-value-redaction])

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
         (step s7 :id configure-data-payment-secrets :logic "Provision DB/migrations, payment product/entitlements when needed, and secret-store refs/env names without writing secret values.")
         (step s8 :id configure-deploy :logic "Configure Vercel/deploy-center, domains, health checks, and release provenance; production migrations remain explicit operations.")
         (step s9 :id register-missiond-universe :logic "Add central project-registry, maturity, service-runtime, and checker entries with management-domain=product-service-layer.")
         (step s10 :id verify :logic "Run project checker, behavior closure, MissionD project universe, runtime compile, contract generation, and maturity gate. Report remaining gaps instead of promoting maturity silently."))
      :egress [repo-scaffold project-local-ssot missiond-registration auth-client db-migrations secret-refs vercel-config verification-report])

    :forbidden-shortcuts
      ["Do not classify product services as xiaojinpro-core-backend just because they call auth/router/payments."
       "Do not put user-facing service code inside MissionD production-system repos."
       "Do not write secret values into generated files."
       "Do not bypass XJP Auth with ad-hoc local JWT parsing for protected products."
       "Do not make payment UI before product_code, entitlement, and webhook verification are declared."
       "Do not run production migrations automatically during cold start."
       "Do not mark a new service M5/M6 without local SSOT, behavior closure, deployment provenance, and regression evidence."]
    :checker "node scripts/check-project-ssot-universe.mjs --engine=ocaml")
