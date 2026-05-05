# M6 Maturity Audit: Infra Service SSOT Batch

- BoardTask: `84295f05-ebf8-41d8-9c23-201401fa28a1`
- Audit date: 2026-05-05
- Scope: `/Users/jinchen/Projects/xiaojinpro-backend` plus registered infra service entries:
  `deploy-center`, `deploy-agent`, `auth`, `router`, `payments`, `asr`,
  `timeline`, and `secret-store-rs`.
- Static-only posture: no production probe, no Cloudflare, no Kubernetes,
  no provider logs, no KB, no Board backlog.
- Allowed evidence used: project roots, `.missiond/**` blueprints/evidence,
  repo-local scripts, and explicit checker output.

## Verdict

All audited entries classify as **M6** for the SSOT maturity contract.

No M6-blocking missing `entry`, `core`, `egress`, `surface`, `runtime`, or
`checker` item was found. No remediation child BoardTasks are required.

`secret-store-rs` is registered outside `/Users/jinchen/Projects` by the
MissionD universe checker at:

`/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs`

## Classification Matrix

| Entry | Root | Maturity | Structure Evidence | Checker Evidence | Missing M6 Items |
|---|---|---:|---|---|---|
| `xiaojinpro-backend` | `/Users/jinchen/Projects/xiaojinpro-backend` | M6 | Project blueprint has 2 functions covering service registry and workspace source hygiene; root intent pins external runtimes for `xjp-deploy-agent` and `secret-store`. | `node scripts/check-xjp-ssot-complete.mjs --json` PASS; universe checker PASS. | None |
| `deploy-center` | `services/deploy-center` | M6 | Backend blueprint has 6 functions with `:entry`, `:core`, `:egress`, `:surfaces`, `:runtime-projection`; covers pipeline, API, auth, agent protocol, executor service, and runtime bootstrap. | `node scripts/check-service-ssot.mjs --all --json` PASS; service checker anchors 7 source paths. | None |
| `deploy-agent` | `crates/xjp-cli` | M6 | Backend blueprint has 3 functions for in-tree operator-client behavior; workspace root `m6-overlay deploy-agent-identity` separates this from the external deploy-target daemon. | `node scripts/check-service-ssot.mjs --all --json` PASS; service checker anchors 6 source paths. | None |
| `auth` | `services/auth` | M6 | Backend blueprint has 25 functions and sidecar shards for domain model, token/session persistence, key rotation, provider regression gates, event taxonomy, route migration, product/admin read models, and decision inbox. | `node scripts/check-service-ssot.mjs auth --json` PASS; aggregate checker runs 15 auth extra checks. | None |
| `router` | `services/router` | M6 | Backend blueprint has 8 functions covering gateway surface, routing, auth, billing, SSE, connector generation, secret-store bootstrap, and service runtime. | `node scripts/check-service-ssot.mjs --all --json` PASS; service checker anchors 7 source paths. | None |
| `payments` | `services/payments` | M6 | Backend blueprint has 8 functions covering domain state, provider adapters, infra/secret loading, order service, credits, fulfillment/outbox, API boundary, and bootstrap. | `node scripts/check-service-ssot.mjs --all --json` PASS; service checker anchors 6 source paths. | None |
| `asr` | `services/asr` | M6 | Backend blueprint has 8 functions covering upload/storage, transcription, alignment, subtitle pipeline, interpreter sessions, auth, jobs/usage, and bootstrap. | `node scripts/check-service-ssot.mjs --all --json` PASS; service checker anchors 5 source paths. | None |
| `timeline` | `services/timeline` | M6 | Backend blueprint has 8 functions covering event/snapshot, project state, branch/patch/review, collaboration, agent surfaces, sharing/membership, assets, and bootstrap. | `node scripts/check-service-ssot.mjs --all --json` PASS; service checker anchors 4 source paths. | None |
| `secret-store-rs` | `/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs` | M6 | Compact intent plus 416-line backend blueprint with 2 pillars and 5 functions; maps API components and 12-table data layer to current code. | `bash .missiond/check.sh` PASS; universe checker PASS. | None |

## Checker Runs

Commands run during this audit:

```sh
cd /Users/jinchen/Projects/xiaojinpro-backend
node scripts/check-xjp-ssot-complete.mjs --json
node scripts/check-service-ssot.mjs --all --json
node scripts/check-service-ssot.mjs auth --json

cd /Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs
bash .missiond/check.sh

cd /Users/jinchen/Projects/missiond
node scripts/check-project-ssot-universe.mjs --json
```

All commands exited `0`.

Observed checker summaries:

- XJP aggregate checker returned `ok=true`, services:
  `deploy-center`, `deploy-agent`, `auth`, `router`, `payments`, `asr`,
  `timeline`, and `diagnostics=[]`.
- Strict per-service checker returned `ok=true` for all seven XJP service
  entries and `diagnostics=[]`.
- Auth-only service checker returned `ok=true`, `diagnostics=[]`, exercising
  the 15 configured auth extra checkers transitively.
- Secret-store checker passed 14 gates: intent existence, canonical root,
  repo anchor, cargo package name, service type, axum version, database,
  cache, crypto deps, pillar presence, API component presence, data-layer
  table anchors, source-layout anchors, and diff-check.
- Universe checker returned `ok=true`, `diagnostics=[]`, with green entries
  for `xiaojinpro-backend`, `deploy-center`, `deploy-agent`, `auth`,
  `router`, `payments`, `asr`, `timeline`, and `secret-store-rs`.

## Structure Notes

### XJP Root

Reviewed files:

- `.missiond/intent.lisp`
- `.missiond/backend/xiaojinpro-backend-blueprint.lisp`
- `scripts/check-xjp-ssot-complete.mjs`
- `scripts/check-service-ssot.mjs`

The root blueprint is a project-level SSOT. It does not need per-service
business functions because those live in service blueprints. Its two functions
cover:

- Service registry projection into MissionD/project-universe.
- Workspace source hygiene and scoped rustfmt discipline.

The root intent also pins two important identity boundaries:

- `xjp-deploy-agent` is an external deploy-target daemon, while the audited
  `deploy-agent` entry is the in-tree `xjp-cli` operator-client head.
- `secret-store` is modeled as an external runtime with checkout
  `/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs`.

### Deploy Center

Reviewed files:

- `services/deploy-center/.missiond/intent.lisp`
- `services/deploy-center/.missiond/backend/deploy-center-backend-blueprint.lisp`

The backend blueprint covers the live infra orchestration surface through six
functions:

- Deployment pipeline trigger/session/agent claim.
- Axum API routing and SSE/log responses.
- Auth extraction for bearer/API key/GitHub OIDC.
- Deploy-agent push and pull/claim protocol.
- Executor service and lifecycle events.
- Runtime bootstrap/config/health.

No missing structural M6 item found.

### Deploy Agent

Reviewed files:

- `crates/xjp-cli/.missiond/intent.lisp`
- `crates/xjp-cli/.missiond/backend/deploy-agent-backend-blueprint.lisp`
- Root `.missiond/intent.lisp` deploy-agent identity overlay.

The audited `deploy-agent` entry is deliberately the in-tree operator client,
not the external daemon. Its blueprint covers three functions:

- Agent CLI loop and deploy-center status/log update.
- Project/quick/log deployment commands.
- Disaster-recovery client surfaces.

No missing structural M6 item found.

### Auth

Reviewed files:

- `services/auth/.missiond/intent.lisp`
- `services/auth/.missiond/backend/auth-backend-blueprint.lisp`
- Auth `.missiond/backend/*.lisp` sidecars
- Auth `.missiond/research/auth-convergence-report-v2.md`
- Auth `.missiond/research/auth-convergence-report-v3.md`

Auth is not a thin map. The backend blueprint has 25 functions and includes:

- Production runtime deployment.
- OIDC discovery/JWKS.
- OAuth authorization-code lifecycle.
- WeChat OAuth state machine.
- Identity provider login unification.
- Product authorization context.
- Token/session result production and token-session persistence gap contract.
- Route token migration matrix.
- Product/admin read model.
- PostgreSQL runtime / MySQL residue retirement.
- Auth EventBus projection and event taxonomy.
- Secret Store JWT key rotation and key-rotation failure modes.
- Tenant/admin/bootstrap boundaries.
- Touched-file rustfmt scope discipline.

The convergence reports still record provider-regression, deploy-smoke, and
user-decision items. Those are represented as checker-pinned decision/gap
contracts, not missing M6 `entry/core/egress/surface/runtime/checker` structure.

No missing structural M6 item found.

### Router

Reviewed files:

- `services/router/.missiond/intent.lisp`
- `services/router/.missiond/connectors.lisp`
- `services/router/.missiond/backend/router-backend-blueprint.lisp`

The router blueprint has eight functions covering request gateway behavior,
routing/load-balancing, auth/API key/JWKS, billing/credits, SSE, provider
connector generation, Secret Store bootstrap, and service runtime.

No missing structural M6 item found.

### Payments

Reviewed files:

- `services/payments/.missiond/intent.lisp`
- `services/payments/.missiond/intent-db-credits.lisp`
- `services/payments/.missiond/backend/payments-backend-blueprint.lisp`

The payments blueprint has eight functions covering domain state, provider
adapters, infra/secret loading, order service, credits, fulfillment/outbox,
API boundary, and bootstrap.

No missing structural M6 item found.

### ASR

Reviewed files:

- `services/asr/.missiond/intent.lisp`
- `services/asr/.missiond/backend/asr-backend-blueprint.lisp`

The ASR blueprint has eight functions covering upload/storage, transcription,
alignment, subtitle workflow, interpreter sessions, auth, jobs/usage/quota, and
runtime bootstrap. Runtime projections cover object storage, Volcengine/router,
NFA/ffmpeg, Redis, auth/JWKS, DB, and Secret Store configuration.

No missing structural M6 item found.

### Timeline

Reviewed files:

- `services/timeline/.missiond/intent.lisp`
- `services/timeline/.missiond/backend/timeline-backend-blueprint.lisp`

The timeline blueprint has eight functions covering event/snapshot writing,
project state, branch/commit/MR/patch/review/comment surfaces, collaboration,
agent/experience surfaces, sharing/membership, assets/cloud-drive, and bootstrap.

No missing structural M6 item found.

### Secret Store

Reviewed files:

- `.missiond/intent.lisp`
- `.missiond/backend/secret-store-backend-blueprint.lisp`
- `.missiond/evidence/m6-convergence-report.md`
- `.missiond/check.sh`

The compact intent declares service type, PostgreSQL, Redis, crypto constraints,
API components, and data-layer tables. The backend blueprint expands those into
five functions:

- `secrets-crud`
- `namespace-management`
- `org-multi-tenancy`
- `auth-endpoints`
- `vault-and-storage`

Every function carries entry, core, egress, surfaces, and runtime projection.
The checker pins current-code anchors including Cargo metadata, axum 0.8,
PostgreSQL/sqlx, Redis, AES-GCM/HKDF/Argon2, API components, 12 concrete
tables in migrations, and required source paths.

Secret-store's own evidence report notes a future touched-file rustfmt checker
as a non-blocking hardening item. This audit does not count that as a missing
SSOT M6 checker item because the project-local M6 checker already proves root,
intent, blueprint, public code anchors, and data-layer anchors. No structural
M6 item is missing.

## Non-Blocking Deferred Items

The following items are recorded by the project SSOTs but do not block M6
classification:

- Auth provider-regression/deploy-smoke/user-decision items: represented in
  auth decision/gap sidecars and covered by auth static checkers.
- Secret-store touched-file rustfmt checker: recorded in secret-store evidence
  as future workflow hardening; SSOT anchor checker coverage is present.
- Heavy build/test/prod validation: intentionally not run in this static audit.

## Decision

`close_or_backfill`: write this report, add Board evidence, and close
BoardTask `84295f05-ebf8-41d8-9c23-201401fa28a1`. No child BoardTasks are
created because all audited entries classify as M6 and no M6-blocking missing
structural/checker item was found.
