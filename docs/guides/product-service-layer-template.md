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
- Next.js `proxy.ts` marker-cookie gate for protected route prefixes.
- Backend JWKS verification for API requests.
- 401 refresh/logout behavior in the frontend API client.

## Payment

Use the shared payments service for product, price, order, subscription, entitlement, and webhook truth.

Declare `product_code`, plans, entitlement policy, webhook verification, refund behavior, and billing region before building payment UI.

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
