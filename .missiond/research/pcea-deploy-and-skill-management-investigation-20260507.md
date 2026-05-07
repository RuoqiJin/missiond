# PCEA Deployment Skills × MissionD Skill-Management Investigation
*Read-only study, 2026-05-07. BoardTask `a086b1e8-5e8a-42dd-9245-abd39ee0aad2`. Context-pack tick `master-tick-000000000344`.*

> Scope reminder: read-only across `~/.claude/skills`, `~/.codex/skills`, `~/.agents/skills`, the two PCEA repos under `~/Downloads/PCEA develop/`, the deploy-center / xjp-deploy-agent paths under `~/Downloads/xiaojinpro-gateway/...`, and `.missiond/workflows` + `.missiond/v3` in this project. Only this report file is written; no PCEA, XJP, or MissionD source is mutated.

---

## 1. Skill Inventory

### 1.1 Roots that actually carry skill content

| Root | Files | Schema | Notes |
|---|---|---|---|
| `~/.claude/skills/` | **90** `SKILL.md` / `skill.md` files across 81 skill folders + 13 service shards (`services/<name>/SKILL.md`) | YAML frontmatter (`name`, `description`, `allowed-tools`, `aka`, `triggers`, `requires`, optional `actions`) + Markdown body with `# INDEX` jump-table convention | Authoritative. Two stray loose `.md` files at root: `gemini-cli.md`, `stardew-assets.md`, `stardew-game.md` (not in any subfolder, no frontmatter — index outliers). |
| `~/.codex/skills/` | 1 directory (`codex-primary-runtime`) — empty | n/a | Skeleton only; nothing to load yet. |
| `~/.agents/skills/` | 12 `apify-*` skill folders, each with `SKILL.md` + `references/` | Same Anthropic-style frontmatter as `~/.claude/skills` | Strict subset of `~/.claude/skills/apify-*`; SHA-compare needed to confirm full duplication, but the directory listing matches `apify-actor-development … apify-ultimate-scraper`. |

### 1.2 Project-local skill artifacts inside the read scope

| Path | Kind | Purpose |
|---|---|---|
| `~/Downloads/PCEA develop/pcea-api/.missiond/intent.lisp` | Lisp SSOT | Project-local intent for `pcea-api` (referenced from MissionD project-blueprint-registry). |
| `~/Downloads/PCEA develop/pcea-video-vault/.missiond/intent.lisp` + `intent-flow-preview-mode.lisp` | Lisp SSOT | Frontend intent + preview-mode flow. |
| `~/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center/.missiond/intent.lisp` + 8 backend shards + `evidence/*.md` | Lisp SSOT + evidence | M6 Auth-grade convergence package (see §3). |
| `~/Projects/missiond/docs/designs/skill-phase{2,3}-*.md`, `skill-self-management.md` | Design docs (not skill files) | Pre-existing MissionD design notes — out of read scope for this report's recommendations but worth cross-checking before implementation. |

### 1.3 Skills directly relevant to PCEA + Deploy

```
.claude/skills/pcea/SKILL.md                  — PCEA video-vault + pcea-api operator manual (≈32K)
.claude/skills/pcea-knowledge/SKILL.md        — Hybrid RAG pipeline (Router→3090Ti embedding → pgvector + FTS RRF)
.claude/skills/deployment/SKILL.md            — Top-level deploy index (servers, MCP tools, build-strategy)
.claude/skills/deploy-ops/SKILL.md            — On-call runbook (CI red, GCP VM oom, config drift cookbook)
.claude/skills/backend-deploy/skill.md        — GA-CI + DC-CD architecture + 18 CI infra pitfalls
.claude/skills/xjp-deploy-center/skill.md     — DC monorepo subservice (5 Ctx + Redis + 3 workers)
.claude/skills/xjp-deploy-agent/SKILL.md      — Independent agent repo, P1-P7 refactor history, RBAC
.claude/skills/deployment-troubleshoot/…      — XjpFS / tunnel / DB cookbook (referenced by deploy-ops)
.claude/skills/services/deploy-center/SKILL.md — Service-shard skill (sibling under services/*)
```

### 1.4 Format observations (input for the indexer)

- Frontmatter uses *both* `name:` and Anthropic SKILL frontmatter conventions (`allowed-tools`, `aka`, `triggers`, `requires`, `actions`). PCEA SKILL even embeds `actions:` with `requires_approval` — these become first-class fields.
- Inline runnable contracts already appear: `backend-deploy/skill.md` contains
  ```workflow id="check_deploy_status" type="sequential"
  steps: …
  ```
  blocks — i.e. the SKILL.md is already a partial executable contract surface. Indexer should preserve these blocks intact.
- Filename casing is mixed (`SKILL.md` vs `skill.md`). Indexer must be case-insensitive.

---

## 2. PCEA Deployment Map

### 2.1 Repos and branches

| Repo | Local path | Remote | Default branch |
|---|---|---|---|
| `pcea-video-vault` (frontend) | `/Users/jinchen/Downloads/PCEA develop/pcea-video-vault` | `xiaojinpro-team/pcea-video-vault` | `main` |
| `pcea-api` (Rust backend + worker) | `/Users/jinchen/Downloads/PCEA develop/pcea-api` | `xiaojinpro-team/pcea-api` | `master` |

Both repos share an ECS box at `/opt/pcea/` and a single `docker-compose.yml` (frontend repo owns the canonical compose; backend repo carries a dev-only compose with `pgvector/pgvector:pg17` while ECS production runs `pg15` — see §7).

### 2.2 GitHub Actions pipelines

`pcea-video-vault/.github/workflows/deploy.yml`:
- `runs-on: [self-hosted, linux, x64, xjp, oss]` (privatecloud-only `oss` label).
- Concurrency group `deploy-pcea-${{ github.ref }}` cancels in-progress.
- Pre-pulls `node:22-alpine` and `nginx:alpine` via Harbor proxy `192.168.1.20:8880/dockerhub-cache/`.
- `docker build --build-arg REGISTRY_PREFIX=192.168.1.20:8880/dockerhub-cache/ -t ghcr.io/xiaojinpro-team/pcea-video-vault:<sha> -t :latest .`
- Pushes to GHCR (`continue-on-error`, backup channel).
- Builds an **artifact bundle** = `image.tar.gz` + `docker-compose.yml` + `deploy.sh` + `deploy-api.sh` + `sql/*` → tar.gz → `mc cp aliyun/rickyjim/deploy-images/pcea-video-vault/pcea-<sha>.tar.gz`.
- Triggers Deploy Center via OIDC (audience `deploy-center`), falling back to `secrets.CI_DEPLOY_API_KEY` for the legacy path:
  `POST https://auth.xiaojinpro.com/api/deploy/ci/trigger/pcea-video-vault`
  body `{"image":"ghcr.io/.../pcea-video-vault:<sha>","commit_hash":"<sha>"}`.
- `docker image prune -f --filter until=72h` cleanup.

`pcea-api/.github/workflows/deploy.yml`:
- Same runner labels, same OIDC pattern.
- No artifact bundle; only `docker save | gzip → /tmp/pcea-api-<sha>.tar.gz → mc cp aliyun/rickyjim/deploy-images/pcea-api/`.
- `POST /api/deploy/ci/trigger/pcea-api` with `{"image":"pcea-api:<sha>",...}` (note: image is a **local tag**, not a GHCR ref — pull happens by `docker load` on ECS, never `docker pull`).

### 2.3 ECS execution scripts (overwritten on every bundle)

`deploy.sh` (frontend pipeline; canonical at `/opt/pcea/deploy.sh`):
1. `mc cp oss-internal/rickyjim/deploy-images/pcea-video-vault/pcea-<TAG>.tar.gz /tmp/pcea-deploy/`.
2. `tar xzf` → `docker load` → `docker tag <name>:<TAG> <name>:latest`.
3. **Self-update**: copies `deploy.sh`, `deploy-api.sh`, `docker-compose.yml`, `sql/*` from the bundle over the live files (immutable deploy core).
4. `docker compose exec -T postgres psql -U pcea -d pcea_db -f /dev/stdin < sql/pond-schema.sql` (best-effort migration).
5. `docker compose up -d --no-deps --force-recreate pcea` (frontend only).
6. `docker image prune -f --filter until=72h`.
7. Health: `curl http://localhost:3002/api/health` && `curl http://localhost:3001/`, 30 × 2 s.

`deploy-api.sh` (Rust backend; same pattern, scoped to `pcea-api` service, image tag `pcea-api:<TAG>`, health `:3002/api/health`).

### 2.4 ECS docker-compose.yml (production canonical)

```yaml
services:
  postgres:    # pgvector/pgvector:pg15, 127.0.0.1:5433→5432, healthcheck pg_isready
  pcea:        # ghcr.io/xiaojinpro-team/pcea-video-vault:latest, :3001
  pcea-api:    # pcea-api:latest (local), :3002, env_file pcea-api.env
               # environment: DATABASE_URL postgres://...@postgres:5432/${POSTGRES_DB},
               # AUTH_BASE=https://auth.xiaojinpro.com, S3_BUCKET=pceatop,
               # S3_ENDPOINT=oss-cn-shanghai-internal, S3_PUBLIC_URL=oss-cn-shanghai,
               # XJP_PAYMENTS_BASE_URL, INTERNAL_API_TOKEN
               # depends_on postgres healthy
               # healthcheck curl /api/health/deep, 30s/5s/3
volumes: pcea_postgres_data (named)
```

### 2.5 PCEA service.manifest.toml (DC manifest-verify gate)

```
name = pcea-api,  deploy_project = pcea,  language = rust
env.required: DATABASE_URL, AUTH_ADMIN_API_KEY, LLM_API_KEY, S3_*, XJP_PAYMENTS_BASE_URL, INTERNAL_API_TOKEN
env.optional: PORT(3002), SITE_BASE, AUTH_BASE, LLM_BASE_URL, LLM_MODEL,
              LLM_POLISH_MODEL, OLLAMA_URL, EMBED_MODEL, EMBED_DIMS
healthcheck:  shallow=/api/health, deep=/api/health/deep
smoke probes: credits_balance (admin GET), knowledge_search (POST {"query":"测试","limit":1})
deps:         postgres tcp $DATABASE_URL,
              auth http $AUTH_BASE/health,
              payments http $XJP_PAYMENTS_BASE_URL/health,
              ollama http $OLLAMA_URL/api/version
```

DC reads this file via `src/manifest.rs::fetch_from_github` + `verify_against` (fail-fast on missing required envs; absent manifest is SKIP).

### 2.6 Deploy Agent project bindings on ECS

`/etc/xjp-deploy-agent/.env` (per `pcea` skill #deploy):
```
PROJECT_PCEA_DIR=/opt/pcea
PROJECT_PCEA_SCRIPT=./deploy.sh        # Script type, not docker_compose
PROJECT_PCEAAPI_DIR=/opt/pcea
PROJECT_PCEAAPI_SCRIPT=./deploy-api.sh
```
DC project slugs `pcea-video-vault` and `pcea-api` ⇄ Agent project names `pcea` and `pceaapi`. The agent never `docker pull`s — only `mc cp oss-internal` + `docker load`.

### 2.7 Deployment-fact lineage

`git push` → GA (privatecloud `oss` runner) → docker build via Harbor cache → bundle/tarball to OSS public endpoint → `POST /api/deploy/ci/trigger/<slug>` (OIDC) → DC creates DeployRun + DeployLog (priority queue, dedupe key) → ECS Agent `claim-next` (`FOR UPDATE SKIP LOCKED`) → run `deploy.sh|deploy-api.sh` → emit `DeployCreated` / `DeployStageStarted` / `DeployStageCompleted` events back into DC. Heartbeat 10 s; lease 60 s reaped by `lease_reaper` worker.

---

## 3. Deploy Center / Deploy Agent Evidence

### 3.1 Deploy Center

Local canonical root: `~/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center`.

Layout (read-only enumeration):
```
Cargo.toml
crates/         dc-dr, dc-mesh, dc-resources
migrations/     0001_init.sql … 0020_executor_instances.sql
src/            api/, db/, mesh/, monitor/, workers/, disaster_recovery/
                auth.rs, cache.rs, deploy_agent.rs, error_extractor.rs,
                executor_service.rs, gitee_sync.rs, github_client.rs,
                manifest.rs, models.rs, oidc.rs, pipeline.rs,
                pipeline_command.rs, session_machine.rs, smoke.rs, state.rs,
                changelog_worker.rs, commit_sync_worker.rs, lib.rs, main.rs
.missiond/      intent.lisp, m10-convergence.lisp, check.sh,
                backend/{8 shards}, evidence/{current-code-mapping.md,
                  m10-final-convergence-report.md, m6-shard-split-report.md}
```

Key facts from `.missiond/intent.lisp`:
- `:port 8090`, `:database postgresql`, `:cache redis`, `:public-issuer https://auth.xiaojinpro.com`.
- Forbidden root: `/Users/jinchen/Projects/xiaojinpro-backend` (the canonical clone is the Downloads copy, not Projects).
- **Deployment authority chain** (verbatim): *"deploy-center is the deployment-fact authority for production services. CI trigger → session dedup → executor assignment → agent pull → heartbeat → complete. Trigger and executor are decoupled so retries never re-trigger CI."*
- Maturity **M6** (Auth-grade), code-aligned, evidence in `current-code-mapping.md` and `m10-final-convergence-report.md`.
- Project-local checker: `bash .missiond/check.sh`; maturity gate: `node ~/Projects/missiond/scripts/check-project-maturity.mjs --engine=ocaml --json --min-level M6 --project deploy-center`.

Surface map (from `dc-implementation-map.lisp`):
- `api/trigger.rs` → `RequiredAuth + tenant-isolation + 30/min rate-limit + SessionsRepo dedup` → `pipeline::create_run_and_build_task` → broadcast `DeployCreated` + `ExecutorAssigned`.
- `api/executors.rs::claim_next_task` → `ApiKeyAuth.can_access_executor + 100/min` → `executor_service::claim_next_for_executor` (`FOR UPDATE SKIP LOCKED`) → lease stamp + `ExecutorStarted` + metadata-diff `AgentSystemAlert`.
- `auth.rs` → JWKS-only RS256 (no per-request introspection); `ApiKeyAuth` writes audit row.
- `manifest.rs` → fetches `service.manifest.toml` from GitHub at the deploy ref, fails fast on missing required env keys, `SKIP` if no manifest.
- `smoke.rs` → executes `[[smoke]]` probes against `fallback_base_url`; flips `deploy_logs.status` on failure.

### 3.2 Deploy Agent

Local canonical root: `~/Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps/xjp-deploy-agent` (this is the in-monorepo working copy; the production source-of-truth is the **independent** repo `RuoqiJin/xjp-deploy-agent` per the skill file).

```
Cargo.toml, Cargo.lock           workspace 10.7.0
DESIGN-AUTO-DISCOVERY-V2.md, DESIGN-LAN-DISCOVERY.md, DESIGN-TASK-RECOVERY.md, REFACTOR_PLAN.md
Dockerfile, Dockerfile.windows   linux + windows targets
docker-compose.yml               local dev only
install.sh, install-macos.sh, uninstall-macos.sh,
com.xiaojinpro.deploy-agent.plist
crates/                          agent-core (zero-tokio domain), agent-infra
                                 (DC client + SQLite + log sink), tunnel-protocol
                                 (MessagePack codec + LZ4 + 30 TunnelMessage variants)
bins/                            xjp-filebox (CutHub file/video/DB API),
                                 xjp-tunnel-server (skeleton)
src/                             actors/, api/ (18 modules), middleware/
                                   (auth + hmac_auth + rbac), domain/, infra/,
                                   ports/, services/, state/
migrations/                      SQLite WAL, 2 migration files
tests/e2e/                       6 contract test skeletons
```

Operational surface (from `xjp-deploy-agent` skill):
- 5 deployments: GCP `:9876`, ECS `:9877`, privatecloud `:9877` + tunnel `:19877`, Windows `:9876` + tunnel `:19878`, BWG = tunnel server.
- SQLite tables: `tasks`, `exec_tasks`, `outbox` (Outbox Pattern: `tasks` update + `outbox` insert in one transaction), `project_configs`, `meta`, `hmac_keys`.
- Actor supervision (ractor 0.15.12, OneForOne, max 3 consecutive restarts): Supervisor → Reporter + Puller + Watchdog. Internal code is forbidden from calling `std::process::exit`; only `stop()`.
- Auth stack: x-api-key (constant-time, multi-value reject) + HMAC-SHA256 (signed canonical `METHOD\npath\nts\nnonce\nsha256(body)`) + RBAC (11 scopes, 5 high-risk endpoints gated).
- Project config two routes: env (`PROJECT_<NAME>_*`) **or** API (`xjp_agent_project_create`). PCEA uses the env route (Script type, not docker_compose).

### 3.3 Maturity registry snapshot (from `.missiond/v3/missiond-blueprint.lisp`)

```
deploy-center : current=M6 target=M6 gap=[]            ;; Auth-grade
deploy-agent  : current=M5 target=M6 gap=[agent-execution-boundary
                                          server-fact-ledger
                                          final-m6-report]
pcea          : current=M6 target=M6 gap=[]            ;; Auth-grade
auth, router  : M6
xiaojinpro-backend : M5 → M6 (gap=monorepo-service-boundary, deploy-fact-authority, …)
```

i.e. PCEA + DC are already Auth-grade; the remaining work in this universe is `xiaojinpro-backend` shedding its monolith and the agent crystallising server-fact ledger.

---

## 4. MissionD Skill-Management Recommendation

The pillar `worker-runtime / skill-runtime` already exists in V3 (`mission_skill_query | mission_skill_context | mission_skill_mutate | mission_skill_exec`, code-aligned to `crates/missiond-daemon/src/handlers/knowledge/skill/{query,context,mutate,exec}.rs` + `crates/missiond-mcp/src/tools/knowledge/skill.rs`). Recommendation below treats *read-time inventory* and *runtime distillation* as two different surfaces that share storage.

### 4.1 Read-time skill inventory (the "library catalog" surface)

**Goal**: every skill file (and project-local SKILL.md / `.missiond/intent.lisp`) is reachable through `mission_skill_query` with FTS + vector hybrid retrieval, without ever mutating the source files.

Boundaries:
- Source roots are **read-only**. MissionD never writes to `~/.claude/skills/**`, `~/.codex/skills/**`, `~/.agents/skills/**`, or any project-local `.missiond/intent.lisp`. Edits to skill files remain a human/agent task in the owning repo.
- Inventory data lives in MissionD's existing PG (or SQLite for the local cache); skill files are the SSOT.
- Watcher fires on `SKILL.md` mtime change (no polling). Failure to embed a chunk fails fast — no silent fallback (matches `feedback_fail_fast_no_fallback`).

Surface contract additions:
- `mission_skill_query(mode={fts5|vector|hybrid|topic|action}, source={claude|codex|agents|project|all}, ...)` — extends the existing tool with explicit retrieval mode and source-root filter.
- `mission_skill_context(skill_id, include_dependencies=true)` — returns the SKILL body, the `requires.skills` closure, and any embedded `## workflows` blocks parsed from the body.
- `mission_skill_mutate` is **not** the right tool to write to `~/.claude/skills/`. Reuse it only for runtime-skill (workflow) registration in MissionD's own DB, not for editing user-owned skill files.

### 4.2 Runtime distillation (the "executable workflow" surface)

**Goal**: lift the methodology lisps under `.missiond/workflows/*.lisp` into MissionD runtime as first-class executable skills. They are already structured contracts (`workflow … :match_rules :steps :risk-gates :completion`) — `mission_skill_exec(workflow=<id>, …)` should be able to drive them through the existing flow_run / BoardTask plumbing without re-encoding them.

Distinction (the requirement in the brief):
- **Read-time inventory** — answers *"is there a skill that talks about X?"* using the on-disk SKILL.md corpus. Outputs: file ref, frontmatter snippet, matching headings, score. Read-only.
- **Runtime distilled skills** — answers *"run the canonical procedure for X."* Outputs: BoardTask + flow_run + cascade trigger receipts. Mutating but bounded by the workflow's own `risk-gates`.

These two surfaces share storage (one `skills` table) but expose different result shapes via a `kind` column (`md-skill` vs `workflow-skill`).

### 4.3 Workflow-skill candidates already in tree

Eight workflows under `.missiond/workflows/` are immediately liftable (each has explicit `:match_rules`, `:steps`, `:risk-gates`, `:completion` — the V3 schema):

```
m6-deployment-rollout         deploy-center first → router → pcea, no auto-rollback
deployment-event-response     accepts deploy-center webhooks, never mutates
project-ssot-convergence      multi-project SSOT muscle-memory
project-m6-depth              referenced from §s8c above
multi-project-m6-wave         parallel multi-project SSOT push
nightly-evolution             observe-only V3 self-review
commit-lisp-convergence       commit → backfill BoardTask
conversation-memory-distillation  observe-only memory candidate report
```

Plus the historical-methodology lisps `bus-refactor`, `pillar-refactor`, `typed-lisp-compiler-{cleanup,convergence}` which should be marked `:status historical-methodology` and indexed without being runnable.

### 4.4 SKILL.md inline workflow blocks

`backend-deploy/skill.md` already embeds:
```
```workflow id="check_deploy_status" type="sequential"
steps:
  - name: 诊断部署状态
    tool: xjp_deploy_diagnose
    params: { project: xiaojinpro-backend }
    save_as: diagnose_result
  …
```
```
The skill-runtime parser should extract these blocks during indexing, register them as `kind=md-workflow-skill` with `source_path` pointing back to the SKILL.md, and expose them via `mission_skill_exec` for parity with the lisp workflows. This converts the current "human reads SKILL.md and copies the steps" pattern into a real execution path without re-authoring anything.

### 4.5 Authority and policy

- `mission_skill_*` is the only egress for skill content; do not bypass to raw filesystem reads.
- Default mode is observe-only for any `mission_skill_exec` whose backing workflow has `:status designed` or `:status historical-methodology`.
- For workflow-skills with deployment side-effects (e.g. `m6-deployment-rollout`), enforce that the **deploy-center remains the deployment-fact authority** (already encoded as `gate g3` in the workflow). MissionD never auto-rollbacks, never mutates DNS or secrets, never edits CI YAML files.
- Permission policy: `mission_skill_exec` follows the existing `permission` pillar (`mission_permission_query/mutate`). Workflow-skills that touch the file system declare `write_scope`/`must_not_touch` exactly like BoardTasks.

---

## 5. FTS5 + Embeddings Indexing Proposal

### 5.1 Source set + dedup

| Bucket | Path glob | Approx count | Notes |
|---|---|---|---|
| `claude` | `~/.claude/skills/**/{SKILL,skill}.md` + the three orphan `*.md` at root | ~90 | Authoritative copy. |
| `codex` | `~/.codex/skills/**/SKILL.md` | 0 today | Reserved for future Codex-primary runtime. |
| `agents` | `~/.agents/skills/**/SKILL.md` | 12 | All `apify-*`. SHA-compare with `claude`; keep the agents copy as a *mirror* row pointing to the claude-bucket id when the body is identical. |
| `project` | `<project_root>/.missiond/SKILL.md`, `<project_root>/SKILL.md`, `.missiond/intent.lisp`, `.missiond/backend/*.lisp`, `.missiond/workflows/*.lisp` | dozens | Project-local skills + structural lisps already in V3. |

Dedup rule: `(source_root, slug)` is unique; `(sha256(body))` rolls into a `skill_dupes` join table so the same skill across roots indexes once but is reachable via every root.

### 5.2 Schema (extend, do not replace, the existing `skills` storage)

```sql
-- One row per concrete file
CREATE TABLE skills (
  id              UUID PRIMARY KEY,
  source_root     TEXT  NOT NULL,           -- claude | codex | agents | project
  slug            TEXT  NOT NULL,           -- e.g. 'pcea', 'services/deploy-center'
  kind            TEXT  NOT NULL,           -- md-skill | md-workflow-skill | lisp-workflow | lisp-blueprint
  abs_path        TEXT  NOT NULL,
  name            TEXT,
  description     TEXT,
  aka_json        JSONB,                    -- ['pcea-video-vault', 'pcea-api', ...]
  triggers_json   JSONB,                    -- ['deploy pcea', '部署 pcea']
  requires_json   JSONB,                    -- {skills:[...], infra:[...], kb:[...]}
  frontmatter_json JSONB,                   -- entire YAML frontmatter
  body            TEXT,
  sha256          TEXT NOT NULL,
  mtime           TIMESTAMPTZ,
  status          TEXT,                     -- active | designed | historical-methodology | deprecated
  UNIQUE (source_root, slug)
);

-- One row per ~400-char body chunk
CREATE TABLE skill_chunks (
  id          UUID PRIMARY KEY,
  skill_id    UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
  ord         INT  NOT NULL,
  heading     TEXT,                          -- nearest H1/H2 heading
  text        TEXT NOT NULL,
  embedding   vector(1024)                   -- qwen3-embedding 7.6B Matryoshka 1024
);
CREATE INDEX skill_chunks_skill_idx ON skill_chunks(skill_id);

-- FTS5 (SQLite local cache) or PG GIN tsvector for the central index
CREATE VIRTUAL TABLE skills_fts USING fts5(
  name, description, aka, triggers, body,
  content='skills', content_rowid='rowid',
  tokenize='unicode61 remove_diacritics 2'   -- Chinese+English mixed
);
```

PG variant: replace `skills_fts` with `tsvector` columns + GIN; the search adapter keeps the same interface.

### 5.3 Indexer (read-only walker)

1. Walk the four roots; parse YAML frontmatter (`name`, `description`, `aka`, `triggers`, `requires`, `actions`, `status`).
2. Strip frontmatter, split body by H1/H2 (`#` / `##`); within each section, slide a 400-char window with 50-char overlap; preserve fenced code blocks intact (do not split inside ```…```).
3. SHA-256 the raw file → upsert `skills` row.
4. Diff `skill_chunks` by `(skill_id, ord)`; only embed new/changed chunks.
5. Embedding via the existing PCEA route — `Router /embedding/api/embed` (Bearer LLM_API_KEY) → BWG tunnel → Windows 3090Ti → `qwen3-embedding:7.6b` Q4_K_M → 4096 → Matryoshka truncate 1024. Reuse the exact pipeline `pcea-knowledge` already runs to avoid a second embedding stack.
6. Fail-fast on embedding error; persist `skill_chunks.embedding=NULL` only when the file is not yet embedded (never silently dropped).
7. Watcher: fs-event for skill roots; debounce 500 ms; trigger re-index on mtime change.

### 5.4 Retrieval

Hybrid search reuses PCEA's RRF approach:

```
query → mission_skill_query(query, mode=hybrid, k=30)
  ├─ FTS5: skills_fts MATCH '{boosted name + description}' → top-50
  ├─ Vector: embed(query) → cosine top-50 from skill_chunks
  └─ RRF merge: score = Σ 1/(60 + rank) → top-30 chunks
       → group by skill_id, keep best-3 chunks per skill
       → resolve to (skill_id, name, description, top headings, abs_path)
```

Filters: `source_root=…`, `kind=…`, `status=…`, `requires.infra contains …`, `triggers contains …`. Frontmatter `aka` and `triggers` are upweighted in FTS (BM25 column weights `name=4, description=3, triggers=3, aka=2, body=1`).

`mission_skill_context(skill_id)` returns: the full body, the embedded `## workflows` blocks, and the closure of `requires.skills` (so a query for `pcea` automatically surfaces `xjp-deploy-agent`, `deployment-troubleshoot`).

### 5.5 What this proposal does **not** do

- It does not export skill bodies to KB by default — KB egress is opt-in per `feedback_no_default_kb_write` and the conversation-memory-distillation workflow.
- It does not edit any skill file or project-local intent.lisp.
- It does not introduce a separate embedding model; reuse Router → 3090Ti.

---

## 6. Distilled Reusable Workflow / Runtime Skills

The three artifact families above (PCEA scripts, DC/Agent code, MissionD workflow lisps) project cleanly into the workflow-skill registry described in §4.

### 6.1 New runtime workflow skills to register

| Skill id | Kind | Source | Purpose |
|---|---|---|---|
| `artifact-bundle-oss-deploy` | workflow-skill | distilled from `pcea-video-vault/.github/workflows/deploy.yml` + ECS `deploy.sh` | Reusable "build → bundle → OSS → DC trigger → agent script" template. Parameters: `image_name`, `repo_owner/repo`, `oss_alias`, `deploy_executor`, `agent_project`, `health_port`, `health_path`, `migration_files`. Replaces the current copy-paste-and-tweak between PCEA frontend, PCEA backend, and any future ECS-bound service. |
| `ga-ci-dc-cd-template` | workflow-skill | distilled from `backend-deploy/skill.md#new-repo` + DC `xjp_deploy_project_create_from_template` | "Onboard a new GitHub repo into GA-CI + DC-CD" — emits the GitHub workflow YAML, calls DC `from-template` (upsert), calls `xjp_agent_project_create`, runs first deploy, watches. |
| `config-drift-grep` | workflow-skill | distilled from `deploy-ops/SKILL.md#配置漂移排查清单` | Read-only diagnostic: `for c in $(docker ps): docker inspect | grep $OLD_HOSTNAME` across all 5 agents; emits findings as a BoardTask, never mutates. |
| `service-manifest-verify-preview` | workflow-skill | distilled from DC `src/manifest.rs` + `pcea-api/service.manifest.toml` | Local preflight: parse `service.manifest.toml`, verify required envs against agent `.env` and Secret Store, dry-run smoke probes. Mirrors what DC will reject at deploy time, but runs locally before push. |
| `pcea-asr-batch-backfill` | workflow-skill | distilled from `pcea/SKILL.md#asr` Phase batch path | Iterate `GET /api/bot/videos?filter=has_audio_no_asr` → `POST /api/bot/asr/batch-transcribe` → poll `GET /api/bot/asr/batch-status/$id`. The procedure is already well-documented; promoting it to a workflow-skill makes batch backfill driveable from `mission_skill_exec`. |

### 6.2 Existing lisp workflows to register as-is

`m6-deployment-rollout`, `deployment-event-response`, `project-ssot-convergence`, `project-m6-depth`, `multi-project-m6-wave`, `nightly-evolution`, `commit-lisp-convergence`, `conversation-memory-distillation` (latter two as `:status observe-only`).

`bus-refactor`, `pillar-refactor`, `typed-lisp-compiler-{cleanup,convergence}` register as `:status historical-methodology` (indexed but not runnable; surfaced for `mission_skill_query` only).

### 6.3 SKILL.md `## workflows` blocks already runnable

- `backend-deploy/skill.md` exposes `check_deploy_status` and `deploy_backend` (`type="sequential"`). These are the canonical "show me the deploy state" / "trigger the backend deploy" recipes the on-call agent already follows by hand.
- The same parser will lift any future `## workflows` blocks added to other skills (no central registration needed).

### 6.4 Read-only SKILLs that should stay catalog-only

`hostvds`, `bwg-vps`, `private-cloud`, `windows-runner`, `aliyun`, `tailscale`, `astrill-gateway`, `xjpfs`, `agent-reach`, `chrome-devtools`, `frontend-design`, `lisp-review`, `claude-api`, `apify-*` (13 sub-skills) — these are operator manuals and reference packs; index them for retrieval but do not synthesise workflow-skills out of them (the procedures are too situational and the methodology lifts above already cover the deploy axis).

---

## 7. Risks / Open Questions

1. **PCEA pgvector version drift.** `pcea-api/docker-compose.yml` (dev) pins `pgvector/pgvector:pg17` while the canonical `pcea-video-vault/docker-compose.yml` (production on ECS) pins `pgvector/pgvector:pg15`. The two compose files are sibling artifacts and the dev one is *not* the one ECS runs (ECS uses the bundle-supplied compose), but a future contributor reading the backend repo could mistake it for production. Recommend annotating the backend compose as `# DEV ONLY — ECS uses pcea-video-vault/docker-compose.yml`.

2. **`agents/` mirroring.** `~/.agents/skills/apify-*` appears to be a strict subset of `~/.claude/skills/apify-*`; if it is meant to be a separate runtime (Codex Agents?), the indexer must surface both rows. If it is just a stale copy, it should be removed at source rather than papered over by dedup. Confirm the intended ownership before treating it as authoritative.

3. **Three orphan files at `~/.claude/skills/` root.** `gemini-cli.md`, `stardew-assets.md`, `stardew-game.md` have no folder and may have no frontmatter. They will index but not render in the YAML-based catalog UI. Decide: move into folders or mark as legacy.

4. **Skills marked deprecated in `CLAUDE.md` but still on disk.** `semantic-terminal` (merged into `missiond`), `minimax` (migrated to Sonnet), `xjp-bff` (deprecated). Index needs a `status=deprecated` tag and the retrieval default should down-rank deprecated skills unless the query explicitly asks for them.

5. **`.missiond/research/` is unindexed and growing.** 22 prior reports (incl. this one) live here. Recommend adding a small `INDEX.md` (or `.lisp`) maintained by `mission_skill_exec` so `mission_skill_query` can reach prior investigations without scanning them on every hit.

6. **Embedding-stack coupling to BWG tunnel.** §5 reuses the PCEA → Router → BWG tunnel → 3090Ti pipeline. If BWG / Windows is offline, skill-index re-embeds will fail fast (per project rule) — that's intentional, but operators need to know skill mutate flows can stall on a Windows reboot. Document this dependency on the `skill-runtime` surface note.

7. **Workflow-skill side-effect classification.** `m6-deployment-rollout` and the new `artifact-bundle-oss-deploy` reach into deploy-center. The split between "MissionD orchestrates" vs "deploy-center decides" is clear in lisp (`gate g3 deploy-center remains release authority`) — but the runtime needs a hard policy gate that prevents `mission_skill_exec` from issuing any direct `docker compose up` / `mc cp` / `git push` outside an explicit BoardTask `write_scope`. Today the policy is implicit; promoting it to `permission-policy` machinery is a prerequisite to enabling these workflow-skills.

8. **Project-local intent.lisp overlap.** `pcea-api/.missiond/intent.lisp` and `deploy-center/.missiond/intent.lisp` are both *project SSOT* (M6) and *skill-like content* (operator-facing). The indexer must route them to `kind=lisp-blueprint` (not `md-skill`) so the surfaces stay separate; otherwise FTS will conflate "PCEA video-vault is M6" and "to deploy PCEA you do X."

9. **Skill `requires.kb`.** Several SKILLs declare `requires.kb: [memory:ops]`. If `mission_skill_context` auto-expands `requires`, the current observe-only memory policy (`feedback_no_default_kb_write`) needs an explicit *read-side* allow rule — context expansion **reads** memory but does not write; verify the existing policy already permits this.

10. **CLAUDE.md index drift.** The `# 全域总纲` index in `~/.claude/CLAUDE.md` lists skills that may not exist on disk (e.g. `palm-era` listed but no folder check was done in this report). The skill index should periodically diff against the CLAUDE.md tables and emit drift warnings, not silently disagree.

---

## Verification

- Report file: `/Users/jinchen/Projects/missiond/.missiond/research/pcea-deploy-and-skill-management-investigation-20260507.md` — single new file, write-scope respected.
- No edits to PCEA repos, `xiaojinpro-backend`, deploy-agent app, or anything else under `~/Projects/missiond` outside the research directory.
- Required sections present: Skill Inventory (§1), PCEA Deployment Map (§2), Deploy Center / Agent Evidence (§3), MissionD Skill-Management Recommendation (§4), FTS5 + Embeddings indexing proposal (§5), Distilled reusable workflow/runtime skills (§6), Risks / Open Questions (§7).
- Concrete file refs: PCEA `.github/workflows/deploy.yml` (both repos), `service.manifest.toml`, `docker-compose.yml`, `deploy.sh`, `deploy-api.sh`; DC `services/deploy-center/{src,migrations,.missiond/{intent.lisp, backend/*, evidence/*}}`; agent `apps/xjp-deploy-agent/{crates, src, migrations, bins}`; MissionD `.missiond/workflows/*.lisp` and `.missiond/v3/missiond-blueprint.lisp`.
- Read-time inventory vs runtime distillation distinction made explicit in §4.1 vs §4.2.
