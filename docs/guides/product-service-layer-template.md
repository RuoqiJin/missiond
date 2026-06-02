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

Use Next.js route handlers only for thin CRUD/BFF APIs. Keep Vite only for existing Vite apps or browser-heavy editor/exporter surfaces.

## Layout

Independent full-stack layout:

```text
frontend/
backend/
  api/axum.rs
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

Default full-stack deployment uses Vercel Services:

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

Vercel does not strip `routePrefix`; axum routes must include `/api/...`.

## MissionD Registration

Register every new service in:

- `.missiond/v3/shards/universe/project-registry.lisp`
- `.missiond/v3/shards/universe/project-maturity.lisp`
- `.missiond/v3/shards/universe/service-runtime.lisp`
- `scripts/check-project-ssot-universe.mjs`

New services default to `incubating-project`, `M2`, target `M6`, with explicit gaps.

Production verification must include auth redirect smoke, Google login redirect smoke, domain readiness, and support mailbox readiness.
