# Auth M6-to-Target Architecture Plan

- BoardTask: `29bbc542-c1c8-424d-8993-3145478906c5`
- Date: 2026-05-05
- Auth root: `/Users/jinchen/Projects/xiaojinpro-backend/services/auth`
- Mode: static-only, no production probes, no Cloudflare, no Kubernetes/live ops, no KB, no historical conversations, no provider durable logs.

## Current Baseline

Auth is M6 for current-code mapping: the SSOT and backend blueprint describe the live service as an OAuth2/OIDC provider with tenant/product/application/user/product-group modeling, provider login routes, token/session persistence, key rotation, admin surfaces, MySQL retirement policy, and MissionD event projection.

The checker spine is green after running the declared static checkers from their expected roots:

```sh
cd /Users/jinchen/Projects/xiaojinpro-backend/services/auth
node ../../scripts/check-service-ssot.mjs auth --json
node ../../scripts/check-auth-event-taxonomy.mjs --json
node ../../scripts/check-auth-decision-inbox-candidates.mjs --json
node ../../scripts/check-auth-key-rotation-failure-modes.mjs --json
node ../../scripts/check-auth-token-session-route-isomorphism.mjs --json
node ../../scripts/check-auth-route-token-migration-matrix.mjs --json
node ../../scripts/check-auth-provider-regression-gates.mjs --json
node ../../scripts/check-auth-product-admin-isomorphism.mjs --json
node ../../scripts/check-auth-token-session-persistence-gap.mjs --json

cd /Users/jinchen/Projects/xiaojinpro-backend
node scripts/check-auth-eventbus-isomorphism.mjs --json
```

All commands above exited 0. `check-auth-eventbus-isomorphism.mjs` expects the monorepo root because it resolves `services/auth/...` paths.

## Evidence Anchors

- SSOT: `.missiond/intent.lisp`, `.missiond/intent-db-identity.lisp`, `.missiond/intent-db-oauth.lisp`, `.missiond/intent-db-session.lisp`, `.missiond/intent-db-iam.lisp`, `.missiond/intent-state.lisp`.
- Backend blueprint: `.missiond/backend/auth-backend-blueprint.lisp`.
- Target context: `.missiond/context-packs/auth-domain-model-v2-context.md`.
- Review evidence: `.missiond/research/auth-architecture-review-v2.md`, `.missiond/research/auth-convergence-report-v2.md`, `.missiond/research/auth-convergence-report-v3.md`.
- Runtime registry: MissionD `.missiond/v3/missiond-blueprint.lisp` project entry for `auth` and `service-runtime-universe` entry for `auth`.
- Source anchors: `src/domain/{product,identity,authz}.rs`, `src/services/{product_access,token_session,missiond_event,key_rotation_service}.rs`, `src/routes/{auth_sms,auth_email,auth_google,auth_wechat,oauth}.rs`, `src/admin/{routes.rs,handlers/products.rs}`, `src/repos/{product_repo,tenant_users_repo,oauth_repo}.rs`.

## Target Gaps

### Tenant / App / Product / User / Group

Current state is strong. `migrations/0007_product_domain.sql` adds first-class `products`, `applications`, `product_users`, `product_groups`, product roles, and permissions. `src/domain/product.rs` and `src/services/product_access.rs` project the runtime facade, and read-only product admin routes exist.

Remaining target gap: the legacy bridges `client_user_access` and `user_service_access` still remain runtime compatibility surfaces. The target plan needs an explicit retirement design that proves when those tables stop being policy inputs, not just migration evidence.

### Login Flows

SMS and email/password/email-code are product-token aligned. Google has handler fixtures and a conditional auth-code token-session bridge. WeChat has strong in-tree fixture coverage but remains behavior-pinned on the production callback smoke item.

Remaining target gap: the provider flow model is distributed across route matrix, provider gates, and convergence reports. The target architecture needs a single login-flow state table that classifies WeChat, Google, SMS, email-password, and email-code across provider verification, identity binding, product access, token-session production, MissionD event emission, and migration eligibility.

### Issuer / Domain

MissionD Universe and auth SSOT agree on `https://auth.xiaojinpro.com` as `public-base-url` and issuer. Runtime registry pins Caddy/Kubernetes facts as Lisp-owned service-runtime data with Cloudflare mutation gated by explicit approval.

Remaining target gap: issuer/domain consistency is checker-pinned but not yet separated into a small route/service split artifact that a future worker can use without reading production deployment files.

### JWT / Key Rotation / Secret Store

Key rotation is M6-described and checker-backed. Secret Store is production authority; DB/manual rotation remains compatibility/emergency. `auth.key.rotation` is code-aligned and sanitized.

Remaining target gap: dual engines are deliberately not merged. The target architecture should define the eventual cutover rule: when Secret Store-only JWT authority can supersede DB/manual rotation, what compatibility fixture proves it, and what admin route behavior remains.

### Admin / Bootstrap

Admin product read models are code-aligned. `ADMIN_MASTER_KEY` remains a visible `requires-user-decision` item, and product write APIs are policy-decided as deferred.

Remaining target gap: bootstrap/admin authority is not yet expressed as a V3-like split between break-glass, deploy-center approval, Secret Store access, admin product reads, future admin product writes, and MissionD visible decisions.

### MySQL Residue

Runtime truth is PostgreSQL. Active MySQL artifacts are retired; historical backup is pinned as evidence.

Remaining target gap: no immediate code task. Future work is a Lisp-only retirement ledger that makes the historical backup inventory and active-forbidden surfaces queryable without re-reading old migration directories.

### Consumer Token Storage

Policy is clear: Auth owns token semantics, validation, issuer, claims, refresh/revoke, and events; browser/mobile/CLI token storage belongs to consuming project blueprints.

Remaining target gap: no auth implementation task. A small Auth boundary artifact should point consumers to the required fields and explicitly refuse browser/CLI storage design inside Auth.

### Event Bus Surface

The event taxonomy is strong: login/admin/token/product/key events are sanitized and checker-backed. Provider callback events remain behavior-pinned, and event envelopes currently use UUID-prefixed per-event ids with a target note for idempotency-key-backed events.

Remaining target gap: V3-like event granularity needs a lifecycle table separating local audit authority, MissionD external-service event projection, idempotency key, retry/failure policy, and route-level producer status.

### Route / Service Split

Auth’s backend blueprint is comprehensive but monolithic. MissionD-V3-like granularity would be easier to maintain if the route groups, service/domain functions, event projection, and migration governance had separate small Lisp shards with stable checker ownership.

Remaining target gap: create design-only shards first. Do not refactor Rust route files before these shards pin route owners, write scopes, acceptance, and behavior-pinned boundaries.

## Recommended Child Shards

1. Route/service split target blueprint.
   - Purpose: create the V3-like decomposition of public routes, admin routes, internal routes, services, repositories, event producers, and migration-governance shards.
   - No Rust changes.

2. Provider login target matrix.
   - Purpose: consolidate WeChat, Google, SMS, email-password, and email-code flow states into one Lisp target matrix, including production-smoke boundary and route-token migration eligibility.
   - No Rust changes.

3. Secrets/JWT/admin bootstrap target boundary.
   - Purpose: define Secret Store authority, DB/manual fallback retirement, key rotation cutover, ADMIN_MASTER_KEY replacement options, and admin product write policy as visible design surfaces.
   - No Rust changes.

4. Legacy bridge and consumer boundary plan.
   - Purpose: define retirement criteria for `client_user_access` / `user_service_access`, MySQL historical evidence posture, and consumer token-storage boundary.
   - No Rust changes.

## Decision

Next action is clear: create visible child BoardTasks for the four Lisp-first design shards above. No direct auth code implementation should start from this parent planning task.
