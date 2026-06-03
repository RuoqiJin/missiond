# Product Service Layer Template

This is the default MissionD template for user-facing services such as Palm Era, Search Center, WePub, ASR, Long Image, Chat Translator, and CutHub.

## Classification

Use `management-domain=product-service-layer` for standalone product services. Do not classify them as `xiaojinpro-core-backend` just because they call auth, router, payments, or secret-store.

Default `runtime-layer`:

- `product-fullstack`: frontend plus backend, default for new services.
- `product-api`: backend/API only.
- `product-frontend`: frontend only, calls existing XJP APIs.

## Repo Placement

Default independent root: `/Users/jinchen/Projects/<project-id>`.

Use `/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/<service-id>` only when the service is tightly coupled to shared XJP crates, deploy-center container runtime, or internal platform service wiring.

## Stack

Default frontend: `Next.js 16 + React 19 + TypeScript + Tailwind 4`.

Default backend: `Rust axum + sqlx + PostgreSQL`.

Rust production builds are not Vercel or target-VM builds by default. Product-service Rust backends use the deploy-center approved privatecloud/codebase build lane, then deploy built artifacts to the runtime target.

Deploy Center build stages for Rust product-service backends use `deploy_type=native_workflow`. The older `docker_build` plus `source_strategy=xjp_native_codebase_runner` shape is migration compatibility only, not the scaffold default.

Use Next.js route handlers only for thin CRUD/BFF APIs. Keep Vite only for existing Vite apps or browser-heavy editor/exporter surfaces.

## Layout

Independent full-stack layout:

```text
frontend/
backend/
  api/axum.rs      # optional, explicit Vercel Rust exception only
  src/lib.rs
  src/main.rs
  src/api.rs
  src/auth.rs
  src/db.rs
  migrations/
.missiond/
  intent.lisp
  backend/<project-id>-backend-blueprint.lisp
  frontend/<project-id>-frontend-blueprint.lisp
  operations/<project-id>-operations-blueprint.lisp
  evidence/<project-id>-final-m6-report.lisp
  behavior-universe.lisp
  check.sh
vercel.json
```

## Auth

Default provider is XJP Auth with PKCE for browser users and service API keys for machine endpoints.

Create OAuth clients through the auth Admin API/MCP surface. Do not edit auth database rows directly.

Every protected product needs:

- Public route list.
- `/auth/login` and `/auth/callback`.
- Browser login must use OAuth2/OIDC authorization-code + PKCE:
  - authorize endpoint: `https://auth.xiaojinpro.com/oauth2/authorize`
  - token endpoint: `https://auth.xiaojinpro.com/oauth2/token`
  - `response_type=code`
  - `code_challenge_method=S256`
  - scope baseline: `openid profile email offline_access`
  - never use legacy `/oauth/authorize` or implicit `response_type=token`.
- OAuth redirect allowlist entries for every production, `www`, compatibility, Vercel/custom, and local development host that can initiate login.
- Next.js `proxy.ts` marker-cookie gate for protected route prefixes.
- Backend JWKS verification for API requests.
- 401 refresh/logout behavior in the frontend API client.

For XJP Auth, create or update clients through `xjp_oauth_client_create`, `xjp_oauth_client_update`, Dynamic Client Registration, or the auth Admin API/MCP surface. Do not write `oauth_clients` rows directly, and do not rebuild or restart Auth just to add redirect URIs.

Default redirect URI set:

```text
https://<canonical-domain>/auth/callback
https://www.<canonical-domain>/auth/callback
https://<compat-domain>/auth/callback
https://<vercel-domain>/auth/callback
http://localhost:<port>/auth/callback
```

Only include hosts that actually exist for the project; do not use wildcard redirect URIs. When a project changes from a compatibility domain to a new brand domain, update the OAuth client before or with the domain cutover.

Auth deploy verification must include a redirect allowlist smoke:

- Compare MissionD `service-runtime` domains, production URLs, and Vercel domains with the OAuth client `redirect_uris`.
- Start login from each live host with `response_type=code` and PKCE, and fail the release on `invalid_client`, `unsupported_response_type`, `invalid_request`, or `Invalid redirect_uri`.
- Start Google login from each live host and fail the release on `invalid_request` / `Invalid redirect_uri`.
- Verify `/auth/callback` returns to the initiating host, sets the expected marker/session state, and reaches a protected route without redirect loops.

## Payment

Use the shared payments service for product, price, order, subscription, entitlement, and webhook truth.

Declare `product_code`, plans, entitlement policy, webhook verification, refund behavior, and billing region before building payment UI.

## Support Mail

Every public product service must declare a support mailbox plan before production promotion.

Default provider is `xjp-mail-service` with Google Workspace as the first physical provider. MissionD owns logical per-service mailbox state; the physical mailbox can be:

- `dedicated_user` for high-value services or strict access boundaries.
- `alias` for low-volume services, with `target_user` declared explicitly.

Default support mailbox provisioning flow:

```text
POST https://mail.xiaojins.com/v1/mail/services/<service_id>/mailboxes/plan
POST https://mail.xiaojins.com/v1/mail/services/<service_id>/mailboxes/apply
GET  https://mail.xiaojins.com/v1/mail/domains/<domain>/readiness
```

The plan step must produce Google Workspace DNS requirements for MX, SPF, and DMARC. DNS mutation must go through `xjp-domain-service`; agents must not directly mutate Cloudflare except through an audited break-glass path.

Default env names:

- `MAIL_API_BASE_URL`
- `MAIL_SERVICE_TOKEN`
- `SUPPORT_MAILBOX_ADDRESS`
- `SUPPORT_MAILBOX_KIND`
- `SUPPORT_MAILBOX_TARGET_USER`

Default agent policy is read/draft-only. Sending requires an approved `mail_agent_actions` row; auto-send is disabled unless the service-specific mail policy explicitly enables it.

## Database

Default independent DB is Supabase Postgres through session pooler port `5432` with `sslmode=require`.

Do not guess the pooler host. Do not run production migrations during Vercel cold start. Migrations must be idempotent.

Shared Supabase projects require table namespacing. Data-bearing services must declare region, data classes, retention, and cross-region default in MissionD SSOT.

## Rust Build Lane

Default Rust backend build lane:

```text
release commit
  -> deploy-center/codebase source sync
  -> privatecloud Rust build
  -> image or binary artifact with digest/provenance
  -> runtime target pull/recreate
  -> health smoke and deploy-center provenance
```

The builder is the deploy-center approved privatecloud/codebase lane, currently represented in MissionD as `privatecloud-10900kf`. This applies even when the runtime target is a GCP VM.

The Deploy Center `stage-configs/build` entry must use `deploy_type=native_workflow`; normal trigger dispatch creates `xjp_workflow_runs` / `xjp_workflow_jobs`, waits for terminal workflow status, then continues to the runtime deploy stage.

Do not run `cargo build`, `docker build`, or `docker compose up --build` on GCP production VMs for product-service Rust backends. A GCP VM deploy stage should only pull or receive an already-built artifact, recreate the service, and report smoke/provenance.

GitHub Actions or Vercel may trigger control-plane events, but they are not the release evidence for Rust backend compilation unless MissionD/deploy-center records an explicit exception. Operator laptop builds are break-glass bootstrap only and need follow-up lane repair.

MissionD project management must show each product-service project's deployment channels from `service-runtime-universe`: privatecloud build lane, runtime target/deploy-center or VM lane, and frontend hosting lane when present.

The canonical deployment-channel shape is build/runtime/frontend:

- Build: `native_workflow` through the privatecloud/codebase builder for Rust product-service backends.
- Runtime: deploy-center/GCP VM/ECS target that consumes an already-built artifact.
- Frontend: Vercel project/domain when a frontend exists.

MissionD may infer legacy XJP monorepo services from `services.yaml` and GitHub workflows, but new product-service projects must declare these channels explicitly in their MissionD/service-runtime facts.

## Deployment Closure Bundle

Before a new product service reaches production deploy, generate the closure bundle:

```bash
node scripts/scaffold-product-deployment-closure.mjs \
  --project-id <project-id> \
  --name "<Product Name>" \
  --domain <frontend-domain> \
  --api-domain <api-domain> \
  --out /Users/jinchen/Projects/<project-id>
```

The bundle creates `service.manifest.toml`, Deploy Center project config, runtime target, DB adoption plan, domain plan, rollback plan, Vercel projection, GCP compose runtime, and `.missiond/check.sh`.

Production deployment is fail-closed if any of these are missing:

- `service.manifest.toml`
- Deploy Center project slug/config
- runtime target
- Secret Store refs
- DB adoption plan
- domain/DNS plan
- rollback artifact plan
- deep smoke and runtime digest observation

Generated GCP compose files must consume immutable image digests and must not contain `build:`. Vercel success, GitHub success, and curl probes are evidence only; deployed status still requires Deploy Center `ReleaseEvidence + ClosureVerdict`.

## Secrets

Secret values never go into Lisp, Markdown, `.env.example`, `vercel.json`, or code.

Use secret refs with this pattern:

```text
projects/<project-id>/<environment>/<SECRET_NAME>
```

Common env names:

- `DATABASE_URL`
- `XJP_AUTH_ISSUER`
- `XJP_AUTH_JWKS_URL`
- `XJP_AUTH_CLIENT_ID`
- `PAYMENTS_API_BASE_URL`
- `PAYMENTS_SERVICE_TOKEN`
- `PAYMENTS_WEBHOOK_SECRET`
- `SECRET_STORE_URL`
- `SECRET_STORE_TOKEN`
- `NEXT_PUBLIC_APP_URL`
- `NEXT_PUBLIC_API_BASE_URL`

## Vercel

Default Vercel deployment is frontend-only. Use Vercel for the Next.js app, production domains, and public env projection; use deploy-center/privatecloud for Rust backend builds and runtime deploys.

Frontend default:

```json
{
  "$schema": "https://openapi.vercel.sh/vercel.json",
  "buildCommand": "pnpm --dir frontend build",
  "installCommand": "pnpm --dir frontend install",
  "outputDirectory": "frontend/.next"
}
```

If a legacy or experimental service still uses Vercel Services, record the exception in MissionD/deploy-center before scaffolding it. Example exception shape:

```json
{
  "$schema": "https://openapi.vercel.sh/vercel.json",
  "experimentalServices": {
    "frontend": {
      "entrypoint": "frontend",
      "framework": "nextjs",
      "routePrefix": "/"
    },
    "backend": {
      "entrypoint": "backend/api/axum.rs",
      "routePrefix": "/api"
    }
  }
}
```

Vercel does not strip `routePrefix`; if an explicit exception uses `experimentalServices` with `routePrefix` `/api`, axum routes must include `/api/...`.

## MissionD Registration

Register every new service in:

- `.missiond/v3/shards/universe/project-registry.lisp`
- `.missiond/v3/shards/universe/project-maturity.lisp`
- `.missiond/v3/shards/universe/service-runtime.lisp`
- `scripts/check-project-ssot-universe.mjs`

New services default to `incubating-project`, `M2`, target `M6`, with explicit gaps.

Production verification must include auth redirect smoke, Google login redirect smoke, domain readiness, and support mailbox readiness.
