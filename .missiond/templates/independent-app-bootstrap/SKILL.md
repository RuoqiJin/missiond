---
name: independent-app-bootstrap
description: Create or update project SKILL.md files and MissionD M6 SSOT scaffolding for independent apps. Use when the user wants a reusable template for new standalone web apps, needs stack/deployment guidance, wants Vercel frontend plus privatecloud/deploy-center Rust backend deployment, wants Supabase/Postgres connected quickly, or wants the project registered in MissionD project universe.
---

# Independent App Bootstrap

Use this skill to create a project-specific `SKILL.md` plus the minimum MissionD-ready SSOT plan for a standalone app.

## Output Contract

Produce these artifacts unless the user narrows the scope:

1. A project skill file:
   - Default Claude-compatible path: `~/.claude/skills/<project-id>/SKILL.md`
   - Default Codex-compatible path when requested: `~/.codex/skills/<project-id>/SKILL.md`
   - If the user asks for an in-repo artifact, put it at `<project-root>/.missiond/skill/SKILL.md` or a requested path.
2. Project-local MissionD SSOT files:
   - `<project-root>/.missiond/intent.lisp`
   - `<project-root>/.missiond/backend/<project-id>-backend-blueprint.lisp` when a backend exists
   - `<project-root>/.missiond/frontend/<project-id>-frontend-blueprint.lisp` when a frontend exists
   - `<project-root>/.missiond/operations/<project-id>-operations-blueprint.lisp`
   - `<project-root>/.missiond/evidence/<project-id>-final-m6-report.lisp`
   - `<project-root>/.missiond/check.sh`
3. MissionD central registration changes when working inside `/Users/jinchen/Projects/missiond`:
   - `.missiond/v3/shards/universe/project-registry.lisp`
   - `.missiond/v3/shards/universe/project-maturity.lisp`
   - `scripts/check-project-ssot-universe.mjs`

Do not put secrets into any skill, Lisp, README, or git-tracked env file. Use names, refs, and env var names only.

## First Pass

Before writing the project skill, collect or infer:

- `project-id`, human name, Chinese aliases, repo root, GitHub repo, production domain, Vercel project.
- App type: CRUD/admin tool, editor/exporter, simulation/game, local-agent bridge, AI tool, marketplace/content app.
- Required surfaces: frontend, backend API, local agent, background jobs, public API, auth, billing, object storage.
- Data classes: public, private user content, SPI, payment ledger, WeChat/local-device data, generated media.
- Deployment target: Vercel frontend only, Vercel frontend + GCP/ECS Rust API, local agent + cloud API, or other.
- Auth choice: XJP Auth, Supabase Auth, private single-user gate, or none for public tools.
- DB choice and region: Supabase shared project, dedicated Supabase project, or no DB.

If facts are missing but a safe default exists, proceed with explicit assumptions in the generated skill. Ask only when the choice changes data residency, billing, auth, production deploy, or secret handling.

## Stack Selection

Default stack for XJP/MissionD standalone apps:

- Frontend: `Next.js 16 + React 19 + TypeScript + Tailwind 4`.
- Backend: `Rust axum 0.8 + sqlx 0.8 + PostgreSQL`, built through deploy-center approved privatecloud/codebase build lane.
- DB: Supabase Postgres, using pooler session mode on port `5432`.
- Auth: XJP Auth with PKCE for user apps; service API key or ingest token for machine endpoints.
- Deployment: Vercel frontend plus deploy-center runtime deployment for Rust APIs. GCP/ECS runtime targets pull built artifacts; they do not compile Rust or run `docker compose up --build`.

Use simpler Next.js full-stack route handlers when:

- The app is mostly CRUD, has no hard server-side domain invariants, and speed matters more than Rust boundaries.
- The backend does not need a reusable Rust library, simulation engine, local agent protocol, or typed domain core.

Use Vite + Express when:

- The app is primarily a browser editor/exporter and existing code is already Vite.
- Server APIs are thin and can run as Node on Vercel or a separate ECS/container target.

Use a local agent when:

- Data lives only on the user's Mac or local network, such as WeChat, files, desktop apps, or private device state.
- Vercel cannot reach the source directly. The agent should push idempotent batches to cloud APIs with a token.

Use Rust backend by default for:

- Games/simulations, payment/auth-sensitive flows, durable event logs, local-agent ingest, strict authorization, API products, or any project intended to become M6 in MissionD.

## Frontend Vercel + Privatecloud Rust Build Pattern

Default: Vercel deploys only the frontend. The Rust backend build goes through deploy-center privatecloud/codebase build lane, then the runtime target pulls/recreates the already-built artifact.

Do not run `cargo build`, `docker build`, or `docker compose up --build` on GCP production VMs for product-service Rust backends. GCP VM is a runtime target, not a Rust builder.

Use a root `vercel.json` for frontend deployment, such as:

```json
{
  "$schema": "https://openapi.vercel.sh/vercel.json",
  "buildCommand": "pnpm --dir frontend build",
  "installCommand": "pnpm --dir frontend install",
  "outputDirectory": "frontend/.next"
}
```

Only use Vercel Services / Rust Functions as a recorded MissionD/deploy-center exception. If a legacy exception exists, the shape may look like:

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

Vercel does not strip `routePrefix`. If an explicit `experimentalServices` exception uses routePrefix `/api`, axum routes must include `/api/...`.

Backend `Cargo.toml` should have a local binary. Add a Vercel function binary only when an explicit exception exists:

```toml
[lib]
name = "<project_snake>_backend"
path = "src/lib.rs"

[[bin]]
name = "<project-id>-backend"
path = "src/main.rs"

# Explicit Vercel Rust exception only.
[[bin]]
name = "axum"
path = "api/axum.rs"
```

Backend layout:

```text
backend/
  api/axum.rs      # optional Vercel exception entry, not default
  src/lib.rs       # build_app, connect_db, shared app state
  src/main.rs      # local dev server; may run migrations locally only
  src/api.rs       # routes and handlers
  src/auth.rs      # auth extractor/client
  src/db.rs        # sqlx pool and query helpers
  migrations/      # idempotent SQL
```

Frontend layout:

```text
frontend/
  src/app/         # App Router
  src/lib/api.ts   # API client
  src/lib/auth*.ts # PKCE/Auth helpers when needed
  src/proxy.ts     # Next 16 auth marker-cookie gate when needed
```

## Rust Build Lane

Required production chain for Rust product-service backends:

```text
release commit
  -> deploy-center/codebase source sync
  -> privatecloud Rust build
  -> image or binary artifact with digest/provenance
  -> runtime target pull/recreate
  -> public and local health smoke
```

Rules:

- The default builder is the deploy-center approved privatecloud/codebase lane (`privatecloud-10900kf` in MissionD SSOT).
- GitHub Actions, Vercel, and local Codex sessions may trigger or inspect the rollout, but they are not the Rust release builder by default.
- Runtime targets such as GCP VM and ECS receive already-built artifacts and report deploy-center provenance.
- Operator laptop builds are break-glass bootstrap only and must create follow-up lane repair evidence.
- Record the build lane, runtime target, artifact digest, smoke URLs, and rollback artifact in the project operations SSOT.

## Supabase Fast Path

Use Supabase pooler session mode for Vercel + sqlx:

```text
postgresql://postgres.<project-ref>:<password>@aws-<n>-<region>.pooler.supabase.com:5432/postgres?sslmode=require
```

Rules:

- Use the exact pooler hostname shown in the Supabase dashboard. Do not guess `aws-0`.
- Use port `5432` session mode for sqlx. Avoid `6543` transaction mode unless the driver is known pgbouncer-safe.
- Avoid direct `db.<ref>.supabase.co:5432` for Vercel because direct connection may be IPv6-only.
- Put `DATABASE_URL` in Vercel env and local `.env`, never in git.
- Keep migrations idempotent: `CREATE TABLE IF NOT EXISTS`, `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`.
- Do not run production migrations during Vercel cold start. Run them through Supabase MCP/CLI or an explicit approved operation.
- For shared Supabase projects, namespace tables with the project concept and document all tables in the project skill.
- For private data or payments, state region partition and cross-region default in MissionD SSOT.

Minimal Rust pool guidance:

```rust
PgPoolOptions::new()
    .max_connections(4)
    .acquire_timeout(Duration::from_secs(10))
    .statement_cache_capacity(0)
```

## Project SKILL.md Template

When creating the project skill, write concise operational truth. Use this shape:

```markdown
---
name: <project-id>
description: <human name and aliases> — <one sentence purpose>. Use for development, deployment, debugging, MissionD SSOT updates, Vercel/Supabase operations, and project-specific architecture decisions for <project-id>.
---

# <Project Name>

## Snapshot

| Item | Value |
|---|---|
| Repo | `<absolute project root>` |
| GitHub | `<owner/repo or unknown>` |
| Production | `<url or not deployed>` |
| Vercel project | `<team/project or unknown>` |
| Supabase project | `<ref/region or none>` |
| Auth | `<XJP Auth / Supabase Auth / private token / none>` |
| MissionD id | `<project-id>` |
| Maturity | `M6` or current target |

## Architecture

Describe the actual runtime in 5-10 bullets. Include frontend, backend, DB, auth, local agent, storage, billing, event/outbox, and deployment boundaries.

## Repo Layout

List the important directories and files. Do not list generated directories such as `node_modules`, `.next`, `target`, or `dist` unless a deployment depends on them.

## Local Development

```bash
# Backend
cd backend
cp ../.env.example .env
cargo run --bin <project-id>-backend

# Frontend
cd frontend
pnpm install
pnpm dev
```

Add exact commands only after checking the repo's real package manager and scripts.

## Environment

List env var names, purpose, and owner. Do not include values.

## Supabase

Document project ref, region, connection mode, table names, migration command, and RLS/ownership assumptions.

## Vercel

Document frontend `vercel.json`, env vars, production domain, and how to trigger/check frontend deployments. Route prefixes for Rust APIs belong in the backend/runtime section unless the project has an explicit Vercel Rust exception.

## Deploy Center / Privatecloud

Document the Rust build lane, privatecloud builder, runtime target, artifact digest/provenance surface, smoke URLs, rollback artifact, and the rule that production GCP/ECS targets do not run `cargo build`, `docker build`, or `docker compose up --build`.

## Auth

Document provider, client id if public, callback URL, token storage pattern, backend verification path, and allowed users/groups.

## MissionD SSOT

List project-local Lisp files, central registry entry, maturity status, and checker command:

```bash
bash .missiond/check.sh
node scripts/check-project-maturity.mjs --engine=ocaml --min-level M6 --project <project-id>
node scripts/check-project-ssot-universe.mjs --engine=ocaml --json
```

## Known Pitfalls

Capture only project-specific pitfalls: privatecloud build lane, target-VM build prohibition, routePrefix exception behavior, Supabase pooler, local agent permissions, region separation, billing boundary, or deployment blockers.

## Current State

State what is live, what is local-only, what is design-only, and what requires user approval before mutation.
```

## MissionD M6 SSOT Template

Create project-local Lisp with these concepts present across files:

- `domain-model`
- `policy-layer`
- `flow-layer`
- `event-contract`
- `event-bus` or `outbox`
- `runtime-projection`
- `implementation-map`
- `code-isomorphism`
- `current-code`
- `compatibility-ledger`
- `hot-path-wiring`
- `regression-matrix`
- `final-m6-report`
- `auth-grade`
- `BoardTask`
- `worker-operational`

Every blueprint function should use:

```lisp
(function <function-id>
  :entry [...]
  :core ((step s1 :logic "...")
         (step s2 :logic "..."))
  :egress [...]
  :surfaces ["..."]
  :runtime-projection (...))
```

Project-local `.missiond/check.sh` should be read-only and cheap. It should verify required files and tokens, not deploy, mutate secrets, or run broad formatters.

## MissionD Central Registration

When adding the project to MissionD:

1. Add `(project :id <project-id> ...)` to `.missiond/v3/shards/universe/project-registry.lisp`.
2. Add `(maturity :id <project-id> :current M6 :target M6 :gap [])` to `.missiond/v3/shards/universe/project-maturity.lisp` only after local M6 checks pass.
3. Add the project checker to `PROJECT_CHECKERS` in `scripts/check-project-ssot-universe.mjs`.
4. If the project `.gitignore` ignores `.missiond/`, remove or narrow that ignore rule.
5. Run:

```bash
bash <project-root>/.missiond/check.sh
node scripts/check-project-maturity.mjs --engine=ocaml --json --min-level M6 --project <project-id>
node scripts/check-project-ssot-universe.mjs --engine=ocaml --json
node scripts/compile-v3-runtime.mjs --json
node scripts/project-v3-contracts.mjs --write --json
node scripts/project-v3-contracts.mjs --check --json
node scripts/check-v3-project-registry-isomorphism.mjs
```

Run `node scripts/check-v3-code-isomorphism-complete.mjs` when the central V3 ABI or typed surfaces changed.

## Guardrails

- Keep the generated skill operational, not promotional.
- Prefer absolute paths for local project roots.
- Match the repo's actual package manager and framework versions.
- Do not invent deployment URLs, Supabase refs, OAuth client ids, or production status.
- Mark unknowns as `unknown` and include the command or UI path needed to discover them.
- Do not deploy, mutate DNS, rotate secrets, or run production migrations unless the user explicitly asks.
- Do not claim `M6` until the project-local checker and MissionD M6 maturity gate pass.
