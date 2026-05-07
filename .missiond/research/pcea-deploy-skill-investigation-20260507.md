# PCEA Deploy & Skill Registry Investigation — 2026-05-07

> Read-only investigation. Evidence-first. No source code modified.
>
> Read scope honored: `~/.claude/skills`, `~/.codex/skills`, `~/.agents/skills`,
> `~/Downloads/PCEA develop/{pcea-api,pcea-video-vault}`,
> `~/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center`,
> `~/Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps/xjp-deploy-agent` (top-level only),
> `.missiond/workflows/`, `.missiond/v3/missiond-blueprint.lisp`,
> `scripts/check-m6-deployment-status.mjs`.
>
> Did not read: KB rows, history rows, provider logs, `crates/**`, `packages/**`.
> No deploy-agent code beyond directory listing (`apps/xjp-deploy-agent/` was not
> explicitly requested as readable for source files; only the skill describing it
> was used; this is noted as a partial coverage gap below).

---

## 1. Skill Inventory

Confidence levels: **A** = directly authoritative for PCEA deploy; **B** =
necessary supporting context; **C** = peripheral but referenced by hop.

### A. Direct authority for PCEA front/back-end deployment

| Skill file | Path | What it owns | Evidence |
|---|---|---|---|
| `pcea/SKILL.md` | `~/.claude/skills/pcea/SKILL.md` | PCEA video-vault + pcea-api dual-pipeline deploy SOP, OSS Artifact Bundle scheme, ECS layout, agent envs, `deploy.sh` semantics, ASR/bot-API runtime, troubleshoot table | `# deploy` section line 70-171 declares the exact GA → OSS → DC → ECS chain; `# docker` block 200-216 lists images/ports/healthchecks; `# git-remotes` 811-816 captures origin/Gitee/GitHub split |
| `pcea-knowledge/SKILL.md` | `~/.claude/skills/pcea-knowledge/SKILL.md` | Hybrid RAG knowledge ingest (qwen3-embedding via Router → BWG → Win 3090Ti), DB schema, ingest API | Lines 27-44 component table; lines 130-142 embedding chain — this is the **same Router/embedding plumbing** that a MissionD skill embedding worker must reuse |

### B. Supporting deploy-system skills (must read together with pcea)

| Skill file | Path | Role for PCEA |
|---|---|---|
| `deployment/SKILL.md` | `~/.claude/skills/deployment/` | Master deploy index; lines 142-150 explicitly delegate ECS/PCEA flow to `pcea` skill; lines 198-220 describe OSS-relay pattern PCEA depends on |
| `deploy-ops/SKILL.md` | `~/.claude/skills/deploy-ops/` | Persistent on-call SOP. Lines 96-138 are the `CI 排障` table; lines 141-177 the `配置漂移排查清单`; the `agent_service_logs` partition cleanup at 215-265 is a known PCEA upstream impact (DC OOM affects ECS callbacks). Also names `xjp_agent_trigger(project="pcea")` as canonical entry |
| `deployment-troubleshoot/SKILL.md` | `~/.claude/skills/deployment-troubleshoot/` | Cross-cutting infra debug (XjpFS, tunnel, MCP key, agent 60s timeout). Required when `pcea` skill's troubleshoot table is insufficient |
| `backend-deploy/skill.md` | `~/.claude/skills/backend-deploy/` | GA-CI + DC-CD nuance. Although primarily for monolith, lines 78-86 enumerate the *three trigger paths* (`webhook` vs `/ci/trigger/` vs `POST /trigger`). PCEA uses path 2 (`/ci/trigger/pcea-video-vault`, `/ci/trigger/pcea-api`) |
| `xjp-deploy-center/skill.md` | `~/.claude/skills/xjp-deploy-center/` | DC architecture (5 Ctx + Redis + 3 Workers); lines 80-95 list the **11 registered slugs** including `pcea-video-vault`; lines 150-160 PCEA-specific OSS-relay config |
| `xjp-deploy-agent/SKILL.md` | `~/.claude/skills/xjp-deploy-agent/` | Agent runtime (P1-P7 phases, SQLite/outbox/actor). Lines 320-338 describe the **Script-type project** (`PROJECT_PCEA_SCRIPT=./deploy.sh`) which is exactly how the ECS agent calls PCEA's deploy.sh |

### C. Infra grounding (consulted on hop)

| Skill | Why it surfaces |
|---|---|
| `aliyun` | OSS bucket `pceatop`, internal/public endpoints, mc alias semantics |
| `hostvds`, `bwg-vps`, `tailscale`, `private-cloud` | Build-host (privatecloud runner) + tunnel/DERP topology |
| `xjpfs` | sccache S3 backend used by GA workflow `--build-arg REGISTRY_PREFIX=` chain |
| `sqlx-cache` | Required for `pcea-api` Dockerfile (Rust + cargo-chef + SQLX_OFFLINE) |
| `auth-persistence`, `xjp-auth` | Bot JWT, AUTH_ADMIN_API_KEY chain referenced in `pcea` skill `# admin` and `# bot-api` |
| `secret-store` | `pcea-api.env` provenance (Volcengine, INTERNAL_API_TOKEN) |
| `services/*` (`~/.claude/skills/services/`) | 16 micro-service sub-skills (auth, deploy-center, router, …). Not PCEA-direct, but the deploy-center sub-skill is the canonical mirror of `services/deploy-center` Lisp/code |

### D. Codex / Agents skill surface

* `~/.codex/skills/` is **almost empty** — only `.system/` and `codex-primary-runtime/` directories exist. No PCEA content, no overlap with `~/.claude/skills/`.
* `~/.agents/skills/` holds **only the Apify family** (12 skills). Symlinked into `~/.claude/skills/` already. Irrelevant for PCEA deploy.

→ **Conclusion**: the only skill home for PCEA deployment knowledge is `~/.claude/skills/`. There is no canonical mirror in MissionD or anywhere else.

---

## 2. PCEA Deployment Map (evidence-driven)

### 2.1 Two services, one ECS box, two pipelines

| Property | `pcea-video-vault` (frontend) | `pcea-api` (Rust backend) |
|---|---|---|
| Local repo | `~/Downloads/PCEA develop/pcea-video-vault` | `~/Downloads/PCEA develop/pcea-api` |
| GitHub remote | `xiaojinpro-team/pcea-video-vault` | `xiaojinpro-team/pcea-api` |
| Default branch | `main` | `master` |
| GA workflow | `.github/workflows/deploy.yml` | `.github/workflows/deploy.yml` |
| Runner label set | `[self-hosted, linux, x64, xjp, oss]` | `[self-hosted, linux, x64, xjp, oss]` |
| Build artifact | **Bundle** (`image.tar.gz` + `docker-compose.yml` + `deploy.sh` + `deploy-api.sh` + `sql/`) → tar gz | Single `pcea-api-{sha}.tar.gz` (image only) |
| OSS path (public alias `aliyun`) | `aliyun/rickyjim/deploy-images/pcea-video-vault/pcea-{sha}.tar.gz` | `aliyun/rickyjim/deploy-images/pcea-api/pcea-api-{sha}.tar.gz` |
| OSS path (ECS internal alias `oss-internal`) | `oss-internal/rickyjim/deploy-images/pcea-video-vault/...` | `oss-internal/rickyjim/deploy-images/pcea-api/...` |
| DC slug | `pcea-video-vault` | `pcea-api` |
| DC trigger endpoint | `POST /api/deploy/ci/trigger/pcea-video-vault` (OIDC, fallback `CI_DEPLOY_API_KEY`) | `POST /api/deploy/ci/trigger/pcea-api` |
| Agent slug (`PROJECT_<NAME>_*` env on ECS) | `PCEA` (work_dir `/opt/pcea`, type `Script`, script `./deploy.sh`) | `PCEAAPI` (work_dir `/opt/pcea`, type `Script`, script `./deploy-api.sh`) |
| Image identity (runtime) | `ghcr.io/xiaojinpro-team/pcea-video-vault:latest` (loaded from bundle, GHCR push is `continue-on-error` backup) | `pcea-api:latest` (local-only tag — not on GHCR; bundle is the only delivery) |
| Compose service | `pcea` (port 3001) | `pcea-api` (port 3002) |
| Postgres | shared `pcea-postgres` (pgvector/pgvector:pg15, host port 5433→5432) | same |
| Healthcheck (container) | `wget http://127.0.0.1:3001/...` | `curl -f http://localhost:3002/api/health/deep` |
| Healthcheck (smoke / external) | `curl http://localhost:3001/` | `curl -sf http://localhost:3002/api/health` (deploy-api.sh L40); `service.manifest.toml` declares `shallow=/api/health`, `deep=/api/health/deep` + 2 smoke probes (`/api/credits/balance`, `/api/knowledge/search`) |
| Service manifest | (none) | `service.manifest.toml` v1 — required env, optional defaults, healthchecks, smoke probes, deps map |

Sources: `pcea-api/.github/workflows/deploy.yml`, `pcea-video-vault/.github/workflows/deploy.yml`, `pcea-video-vault/deploy.sh`, `pcea-video-vault/deploy-api.sh`, `pcea-video-vault/docker-compose.yml`, `pcea-api/service.manifest.toml`, `~/.claude/skills/pcea/SKILL.md` (`# deploy` and `# pcea-api`).

### 2.2 Frontend / backend role decomposition

`pcea-video-vault` ships **all of**:
1. The Next.js 16 static-export bundle (image tag `ghcr.io/xiaojinpro-team/pcea-video-vault`, container `pcea-app`, port 3001).
2. The **shared** `docker-compose.yml`, **`deploy.sh` and `deploy-api.sh` themselves** (they are bundled into the artifact and overwrite their counterparts on ECS — “self-updating immutable deploy”), and the `sql/` migrations directory.
3. The Postgres pgvector definition.

So **`pcea-video-vault`'s pipeline owns the compose + scripts + DB schema authority**; `pcea-api`'s pipeline only updates the Rust container image. This explains why ECS path is `/opt/pcea/` for *both* projects — there is one compose file, two image-update scripts.

`pcea-api` ships:
1. The Axum 0.8 + SQLx + pgvector Rust binary as `pcea-api:latest` (local-only — not GHCR).
2. Its own `service.manifest.toml` SSOT for required envs + healthchecks + smoke probes.
3. Migrations live in `pcea-api/migrations/` but are not copied to ECS by `deploy-api.sh`; the `pcea` skill notes (line 408) that migration 008 was applied **manually** with `psql -f`. This is a real gap (see §4).

### 2.3 End-to-end flow (verified against deploy.yml + deploy.sh)

```
git push main (vault)                                git push master (api)
   │                                                       │
   ▼                                                       ▼
GA self-hosted [xjp,oss] runner                  GA self-hosted [xjp,oss] runner
  • docker build (Harbor proxy cache)              • docker build (cargo-chef + REGISTRY_PREFIX)
  • docker save | gzip → image.tar.gz              • docker save | gzip → pcea-api-{sha}.tar.gz
  • bundle = image + compose + deploy*.sh + sql    • mc cp aliyun/rickyjim/deploy-images/pcea-api/
  • mc cp aliyun/rickyjim/deploy-images/pcea-...   │
  • OIDC token (audience=deploy-center)            • OIDC token (audience=deploy-center)
  • POST /api/deploy/ci/trigger/pcea-video-vault   • POST /api/deploy/ci/trigger/pcea-api
                       │                                         │
                       └────────────────┬────────────────────────┘
                                        ▼
                  Deploy Center  (GCP, /opt/xjp-deploy-center)
                    • OIDC JWKS verify  (fallback: CI_DEPLOY_API_KEY)
                    • build stage DISABLED → enqueue deploy stage
                    • executor = ecs-agent, project = pcea / pceaapi
                                        ▼
                  ECS xjp-deploy-agent (Script type)
                    • mc cp oss-internal/.../pcea-{sha}.tar.gz   (~76 MiB/s)
                    • tar xzf → docker load → docker tag :latest
                    • (vault) overwrite deploy.sh / deploy-api.sh / docker-compose.yml / sql/
                    • docker compose up -d --no-deps --force-recreate <service>
                    • docker image prune --filter until=72h
                    • health loop 30×2s on 3001 / 3002
                                        ▼
              Deploy Center records deploy_log row (success / failed, commit_hash, target_image)
```

### 2.4 The Volcengine ASR + Bot API surfaces (deploy-relevant)

Although orthogonal to the deploy chain, these affect deploy correctness:

* `pcea-api.env` (NOT `.env`) is the source of truth for `VOLCENGINE_*`, `INTERNAL_API_TOKEN`, `XJP_PAYMENTS_BASE_URL`. `deploy.sh` only overwrites `docker-compose.yml`, never `pcea-api.env` (skill line 404). This is a legitimate "data file lives outside immutable bundle" pattern.
* Bot JWT (`bot_hePEYlZco8pJFokNF1GBi8si3lkAEArA`) is the auth path used by `xjp_agent_trigger` and any post-deploy smoke probe; rotation requires `Profile → My Bots` on `pcea.top`.

---

## 3. Deploy Center / Deploy Agent Evidence

### 3.1 Deploy Center is the deployment-fact authority

* **Endpoint surface**: `services/deploy-center/src/api/`. Confirmed files: `agents.rs`, `agent_logs.rs`, `build_strategy.rs`, `commits.rs`, `config_health.rs`, `disaster_recovery.rs`, `events.rs`, `executors.rs`, `github.rs`, `image_gc.rs`, `k8s.rs`, `logs.rs`, `mesh.rs`, `projects.rs`, `resources.rs`, `status.rs`, `trigger.rs`, `webhook.rs`.
* **Provenance API**: `src/api/status.rs`
  * `GET /api/deploy/status` (lines 92-153) → `{healthy, summary, recent_deployments[]}` — exactly what `scripts/check-m6-deployment-status.mjs:fetchDeployStatus` consumes (and what `m6-deployment-rollout.lisp` step `s2` queries).
  * `GET /api/deploy/provenance/:project` (lines 162-240+) → `DeployProvenanceResponse` with `deployed_commit, target_image, target_digest, reported_digest, workflow_run_id, workflow_conclusion, workflow_url, diagnostics`. This is the **non-MissionD release authority** that `project-registry-reconciliation.lisp` and `m6-deployment-rollout.lisp` rely on (gate `g3` in both).
* **Project model**: migration ladder `0022..0041` shows the schema authority for: executor concurrency, deploy retry, deploy_priority, workflow_runs, heartbeat_metadata, exec_task_logs, pending_directives, lease_and_attempts, executor_instances UNIQUE, agent_service_logs partitioning, deploy_changelogs, auth_event_outbox_deliveries, **deploy_event_relay_state (0040)**, and **deploy_agent_update_provenance (0041)**.
* **Migration 0041** explicitly records: *“deploy-center is the authority for agent update runtime facts. MissionD receives relayed event envelopes, but this table remains the source record.”* This is the same boundary `deployment-event-response.lisp` enforces (`g3: deploy-center provenance remains the release authority; MissionD event logs are cache, visibility, and workflow triggers`).

### 3.2 Deploy Agent (independent repo `RuoqiJin/xjp-deploy-agent`)

Top-level evidence inside `apps/xjp-deploy-agent/`:
* `bins/` — `xjp-filebox` (file/video/DB API) + `xjp-tunnel-server` (skeleton).
* `crates/` — `agent-core`, `agent-infra`, `tunnel-protocol` (per skill, not opened in this investigation).
* `migrations/`, `tests/e2e/`, `Dockerfile{,.windows}`, `docker-compose.yml`, `com.xiaojinpro.deploy-agent.plist`, `install*.sh`, `scripts/` — confirms the workspace shape described by the skill.
* `DESIGN-AUTO-DISCOVERY-V2.md`, `DESIGN-LAN-DISCOVERY.md`, `DESIGN-TASK-RECOVERY.md`, `REFACTOR_PLAN.md` — design docs co-located with code.

PCEA-relevant pieces (from skill, not re-opened):
* Agent project type **Script** + `PROJECT_PCEA_SCRIPT=./deploy.sh` is the only sanctioned way to call PCEA's bundle script. Tradeoff: Script-type means *no canary* — there is just `docker compose up -d --force-recreate`, then a 60-second wall-clock health loop in `deploy.sh`. Backend-deploy's canary `-p 19999:HEALTH_PORT` rule does NOT apply here.
* Outbox table + ReporterActor → DC callbacks (carries deploy_log status) → `deploy_logs` row + `deploy_event_relay_state` (mig 0040) → relayed into MissionD EventBridge.

### 3.3 SSOT (per-service manifest) precedent

`pcea-api/service.manifest.toml` shows the **most mature** project-level SSOT in the four read-scope repos: it declares `name`, `deploy_project`, `language`, `[env.required]`, `[env.optional]`, `[healthcheck]` (shallow vs deep), `[[smoke]]` probes, and `[deps]` (kind=tcp|http, url_env, path). DC currently does *not* read this file (no reference in `services/deploy-center/src/`), but it is the obvious next contract: every smoke that can be run against an arbitrary repo without external knowledge can come from the manifest, and the deep healthcheck can iterate `[deps]` rather than be hand-coded.

### 3.4 MissionD-side script `scripts/check-m6-deployment-status.mjs`

Reads `.missiond/v3/missiond-blueprint.lisp` → finds projects whose `:current` is `M6` → cross-checks against a **hard-coded `DEPLOYMENT_MAP`** (lines 22-68) that lists deploy slugs, repo paths, and "M6-relevant subpaths" per project. For PCEA, this map is:

```js
pcea: {
  slugs: ['pcea', 'pcea-api', 'pcea-video-vault'],
  components: [
    { slug: 'pcea-api',         repo: '~/Downloads/PCEA develop/pcea-api',         paths: ['.'] },
    { slug: 'pcea-video-vault', repo: '~/Downloads/PCEA develop/pcea-video-vault', paths: ['.'] },
  ],
}
```

For each component it: (a) fetches `/api/deploy/provenance/<slug>`, (b) git-diffs `${commit}..HEAD -- <paths>`, (c) classifies `deployed-current` / `deployed-stale` / `not-confirmed`. This is PCEA's **only** end-to-end provenance check today. It's evidence-driven and DC-respectful, but the map is hard-coded JS — **the same facts already live in the `pcea` skill** (repo paths, slugs, branch). That's the duplication §5 will eliminate.

---

## 4. Gaps / Risks (where deploy facts aren't first-class)

| # | Gap | Where | Severity | Why it matters |
|---|---|---|---|---|
| G1 | **Skill ≠ DB** — PCEA deploy SSOT lives in a single 860-line markdown file (`pcea/SKILL.md`). No machine-readable mirror exists. | `~/.claude/skills/pcea/SKILL.md` | High | Any worker that hasn't been pre-loaded with this file is blind. `mission_skill_query` already exists in v3, but no skill registry row is materialized for `pcea`. |
| G2 | **Hardcoded DEPLOYMENT_MAP** in `scripts/check-m6-deployment-status.mjs` | script lines 22-68 | High | The map duplicates information that already exists in the `pcea` skill front-matter (`aka`, `triggers`) and in DC's project list. New M6 projects require manual JS edits. |
| G3 | **service.manifest.toml not consumed** | `pcea-api/service.manifest.toml` exists; DC has no reader | Medium | The manifest is the perfect deep-healthcheck + smoke-probe source. DC currently relies on `health_path` configured per-agent-project. |
| G4 | **DB migrations on ECS are manual** | `pcea` skill line 408: "`sqlx migrate!` 没在 main.rs 里调用" | Medium | New PCEA migrations (e.g. 008_asr_transcripts.sql) require human `psql -f`. `deploy.sh` only runs `sql/pond-schema.sql` (vault bundle) — orthogonal to `pcea-api/migrations/`. |
| G5 | **Skill claims Gitee remote, ECS deploy.sh has none** | `pcea` skill line 53 mentions Gitee fallback; `deploy.sh` has zero git invocations | Low (drift) | Skill says "Gitee 已废弃 ECS 不再依赖 git pull" but `# git-remotes` table at the bottom still recommends `origin → Gitee`. Deploy is now zero-git on ECS — the skill section is stale lore. |
| G6 | **`pcea-api:latest` is local-only** | `pcea-api` deploy.yml has no `docker push`; vault deploy.yml pushes GHCR with `continue-on-error: true` (a *backup*, not a source) | Medium | If the bundle ever fails to land on OSS, there is no fallback registry for `pcea-api`. Front-end has GHCR as backup; backend doesn't. |
| G7 | **Skill files aren't versioned per-project** | All skill files share the global `~/.claude/skills/` namespace, no `project_id ↔ skill_id` link table | Medium | When `mission_project.reconcile` runs, there is no formal way to say "the `pcea` project owns these skill files". |
| G8 | **Tunneled tunnel — no ECS in `tunnel/SKILL` table** | `xjp-deploy-agent` skill `# tunnel` block lines 707-715 list BWG/GCP/privatecloud/Windows/ECS with ECS as `client`. But `deployment-troubleshoot/SKILL.md` `# firewall` only documents privatecloud ufw rules. | Low | If ECS tunnel breaks the deploy fail mode is silent (60s timeout); recovery procedure is missing. |
| G9 | **DEPLOY_CENTER_PUBLIC_BASE_URL hard-coded** | `scripts/check-m6-deployment-status.mjs:20` defaults to `https://auth.xiaojinpro.com` | Low | A blue-green DC migration would invalidate the script. Should come from `mission_project(action=universe)`. |
| G10 | **No skill freshness signal** | All skill files have `mtime` only; no "last-verified-against-runtime" stamp | Medium | Skill drift is invisible. The `pcea` skill TODO list (line 842) is partially stale — items checked, but no automated verification. |

---

## 5. MissionD Skill Registry Design

Goal: turn `~/.claude/skills/*` into queryable, reconcilable, project-aligned authority **without** breaking the principle that skill bodies remain Markdown SSOT for humans.

### 5.1 Where it fits in v3

The blueprint already provisions a `skill-runtime` surface (lines 3235-3245) implemented in
`crates/missiond-daemon/src/handlers/knowledge/skill/{query,context,mutate,exec}.rs`. So the **destination is fixed**: this proposal is to feed that surface, not rebuild it.

The blueprint also references *“FTS/vector ranking, topic hit recording, workflow action projection, embedding refresh through ProcessSkillTopic”* — which means an embedding pipeline already exists in design but is not yet fed by skill-file ingestion.

### 5.2 Proposed `skill-registry.lisp` SSOT (additive, not replacing the v3 surface)

```lisp
(skill-identity-contract
  :schema "missiond.skill-identity-contract.v1"
  :fields [skill_id name file_path description type triggers aka requires-skills
           requires-infra owners related-projects related-deploy-slugs
           body_sha256 frontmatter_yaml content_chunks_count last_seen_at status]
  :rule "MissionD is skill registry authority; ~/.claude/skills/<name>/SKILL.md
         remains the canonical body source. The registry mirrors metadata,
         not narrative."
  :reconcile-action mission_skill.reconcile)
```

### 5.3 Skill row schema (PG-ready, derived from frontmatter we already saw)

```sql
CREATE TABLE skills (
  skill_id          TEXT PRIMARY KEY,                -- equal to dir basename, e.g. "pcea"
  name              TEXT NOT NULL,                   -- frontmatter `name`
  file_path         TEXT NOT NULL UNIQUE,            -- absolute, with mtime check
  source            TEXT NOT NULL,                   -- 'claude'|'codex'|'agents'|'project-services'
  description       TEXT,                            -- frontmatter `description`
  aka               TEXT[],                          -- frontmatter `aka`
  triggers          TEXT[],                          -- frontmatter `triggers`
  allowed_tools     TEXT[],                          -- frontmatter `allowed-tools`
  requires_skills   TEXT[],                          -- frontmatter `requires.skills`
  requires_infra    TEXT[],                          -- frontmatter `requires.infra`
  requires_kb       TEXT[],                          -- frontmatter `requires.kb`
  body_sha256       TEXT NOT NULL,                   -- of full file
  frontmatter_jsonb JSONB,                           -- whole front-matter
  status            TEXT NOT NULL DEFAULT 'active',  -- active|archived|stale
  last_seen_at      TIMESTAMPTZ NOT NULL,            -- mtime of file at last reconcile
  registered_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- chunks (for FTS5/embedding; see §6)
CREATE TABLE skill_chunks (
  chunk_id          BIGSERIAL PRIMARY KEY,
  skill_id          TEXT NOT NULL REFERENCES skills(skill_id) ON DELETE CASCADE,
  section_anchor    TEXT NOT NULL,                   -- '# deploy', '# bot-api', '# troubleshoot'
  ordinal           INT NOT NULL,                    -- 0..N within section
  body              TEXT NOT NULL,
  body_tsv          TSVECTOR,                        -- generated, see §6
  embedding         VECTOR(1024),                    -- qwen3 1024 (Matryoshka)
  body_sha256       TEXT NOT NULL,
  updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- per-project alignment (joins ProjectIdentity ↔ Skill)
CREATE TABLE project_skill_link (
  project_id        TEXT NOT NULL,                   -- FK to mission_project
  skill_id          TEXT NOT NULL REFERENCES skills(skill_id) ON DELETE CASCADE,
  role              TEXT NOT NULL,                   -- 'primary'|'support'|'infra'
  source            TEXT NOT NULL,                   -- 'frontmatter'|'manual'|'reconcile-inferred'
  evidence          JSONB,                           -- e.g. {"reason":"frontmatter aka contains pcea-api"}
  PRIMARY KEY (project_id, skill_id, role)
);
```

### 5.4 Alignment with `project-identity-contract` (blueprint line 1202-1206)

The contract already defines fields: `project_id, canonical_root, repo_remote, ssot_paths, deploy_center_slug, forge_project_name, service_ids, aliases, status`.

The **right join** is:
* `project_skill_link.project_id` → `project-identity-contract.project_id`.
* `skills.aka[]` and `skills.triggers[]` are reconciled against `project-identity-contract.aliases[]` and `service_ids[]`. Example: skill `pcea` has `aka:[pcea-video-vault, pcea-api, pcea.top]`. Reconciler emits two `project_skill_link` rows (`role=primary`) — one for project_id=`pcea-video-vault`, one for `pcea-api` (or one project_id=`pcea` with two `service_ids`, depending on how the registry models PCEA — see Decision Inbox item below).

**Decision needed (Decision Inbox candidate — not for this report)**: is PCEA *one* MissionD project (`pcea`) with two service_ids, or *two* projects (`pcea-api`, `pcea-video-vault`)? The check-m6 script and deploy-center treat them as two projects; the skill treats them as one. Reconciler must surface this rather than pick.

### 5.5 Reconcile flow (additive to `mission_project.reconcile`)

```
mission_skill.reconcile
  s1 walk ~/.claude/skills, ~/.codex/skills, ~/.agents/skills, services/<svc>/.missiond/intent.lisp dirs
  s2 parse YAML frontmatter + chunk by `^# ` headers
  s3 compute body_sha256; if unchanged since last_seen_at: skip
  s4 upsert skills row + skill_chunks rows
  s5 enqueue ProcessSkillTopic(skill_id) → embedding worker (qwen3 via Router)
  s6 derive project_skill_link from frontmatter `aka`/`triggers`/`requires.kb`
  s7 emit drift items: {orphan_skill (no project), orphan_project (no skill),
       stale (mtime older than N days and project recently changed),
       broken_requires (references missing skill)}
```

### 5.6 Authority boundary

| Authority | What it owns | What it does NOT touch |
|---|---|---|
| **Filesystem** (`~/.claude/skills/<name>/SKILL.md`) | The full Markdown body (human SSOT) | Anything queryable |
| **MissionD `skills` / `skill_chunks` / `project_skill_link` tables** | Indexed metadata, chunks, embeddings, project links, drift signals | The Markdown body is *cached* not authoritative |
| **`project-identity-contract`** (existing) | `project_id, canonical_root, deploy_center_slug, ssot_paths` | Skill metadata |
| **deploy-center `/api/deploy/provenance/:project`** | Release facts | Skill metadata |

This matches the v3 blueprint's existing claim (line 1205): *"MissionD is project identity and SSOT registry authority; deploy-center is deployment fact authority; Forge is component/pattern/reality catalog authority."* — Skill becomes a fourth axis under MissionD's identity authority, not a separate authority.

---

## 6. FTS5 + Embedding Query Design

### 6.1 Backend choice — PG, not SQLite

Per memory `feedback_drop_sqlite`: *"SQLite 已弃用，只用 PG。"* So **FTS5 (SQLite) is not an option**. Use Postgres `tsvector` + `pg_trgm` + `pgvector` (the same triple PCEA uses for `knowledge_chunks`). Naming the section "FTS5" in the prompt was a generic shorthand; the actual implementation is PG-native.

### 6.2 Indexed fields (per skill row + per chunk)

| Layer | Field | Index |
|---|---|---|
| `skills` | `name`, `description`, `aka[]`, `triggers[]` | `tsvector` GIN (concatenated, weighted: name=A, description=B, aka/triggers=C) |
| `skills` | frontmatter values | `frontmatter_jsonb` GIN (`jsonb_path_ops`) |
| `skill_chunks` | `body` | `body_tsv tsvector` GENERATED, GIN |
| `skill_chunks` | `body` | `pg_trgm` GIN (for prefix/typo on Chinese-mixed code) |
| `skill_chunks` | `embedding` | `ivfflat (vector_cosine_ops)` *or* `hnsw (vector_cosine_ops)`; pick HNSW if pgvector ≥0.5 |

For Chinese tokenization, default `to_tsvector('simple', body)` is too coarse but acceptable for v1. Suggest `pg_jieba` extension as a follow-up shard (don't bundle into the first MR).

### 6.3 Chunking strategy (mirrors `pcea-knowledge`)

`pcea-knowledge/SKILL.md` line 41 defines: *"每 4 条精修字幕为一块 (`INGEST_CHUNK_SIZE = 4`)"*. Reuse that idea for skill chunks, but bounded by **section anchors** (`^# `) — never cross sections. Within a section, chunk paragraphs into ~400-token blocks. Two reasons to keep section anchors:

1. The skill has dense INDEX tables (e.g. `pcea` skill `# INDEX`) that map intents to anchors. `mission_skill_context(skill_id="pcea", section="deploy")` should return only `# deploy` chunks plus the global front-matter.
2. The blueprint already mentions *"section anchor"* implicitly via the INDEX tables — chunking by section preserves the human navigation contract.

### 6.4 Embedding pipeline

Qwen3-embedding via the **same Router proxy** as PCEA (skill `pcea-knowledge` lines 130-142):

```
mission_skill.reconcile  →  ProcessSkillTopic(skill_id)
  →  embedding_worker (Sonnet-gated topic extraction → text)
  →  POST {OLLAMA_URL}/api/embed   (Bearer LLM_API_KEY)
  →  Caddy → Router :8082 (feature: embedding_api)
  →  EMBEDDING_SERVICE_URL = http://104.194.81.38:19434  (BWG tunnel)
  →  Windows 3090Ti :11434  →  qwen3-embedding 7.6B Q4_K_M
  →  4096-dim → Matryoshka truncate → 1024-dim → vector(1024)
```

Reuse the existing `LLM_API_KEY` + `OLLAMA_URL` plumbing (PCEA already proves it works at scale). **Do not** add a second qwen endpoint.

Per memory `feedback_no_fallback_embedding`: no fallback to a smaller model on Windows downtime — fail-fast with explicit `embedding_pending` chunk state and a Decision Inbox item. This matches the global no-fallback principle (per memory `feedback_fail_fast_no_fallback`).

Per memory `extract_conv_topics_llm` rule: topic extraction for skill chunks must use **Sonnet**, not MiniMax. (Same root cause: MiniMax leaks 35% raw code/tool-calls into topic vectors, killing embedding quality.)

### 6.5 Hybrid retrieval (RRF) — same shape as PCEA knowledge

```
mission_skill_query(query, project_id?, role?, limit=30)
  ├─ FTS path:  ts_rank_cd(body_tsv, websearch_to_tsquery(query)) × weight_field
  ├─ Vector path: cosine_distance(embedding, qwen_embed(query))
  ├─ Optional project filter: JOIN project_skill_link USING (skill_id) WHERE project_id = $1
  └─ RRF: score = Σ 1/(60 + rank), top-K
```

For the M6 deploy use-case the call is: `mission_skill_query(query="deploy pcea-api", project_id="pcea-api", role="primary")` → returns the `# deploy` and `# pcea-api` chunks of `pcea/SKILL.md` first.

### 6.6 Reranker reservation

Reserve a stage between RRF and final ranking. Suggested:
* HTTP POST to Router `/embedding/api/rerank` (same auth as embed); model `bge-reranker-v2-m3` 568M (fits on 3090Ti alongside qwen3-embedding 7.6B).
* Fail-open (skip rerank, keep RRF order) per global no-fallback principle — but reranker absence is a *missing capability*, not an error: the worker logs `reranker_unavailable=true` and proceeds. This is one of the few defensible non-fallback "graceful skips" because reranker is purely a quality booster, not a correctness guard. **Open for user decision.**

### 6.7 Incremental update

* Filesystem watcher (`notify` crate) on `~/.claude/skills/`, `~/.codex/skills/`, `~/.agents/skills/`, plus `services/<svc>/.missiond/intent.lisp`.
* On change: enqueue `mission_skill.reconcile(skill_id=...)` — the same code path as full reconcile, but scoped to one skill_id.
* Daily cron: full reconcile to catch out-of-band edits and missing-file detection.

### 6.8 Read-only “FTS only” fallback for fresh installs

Until embeddings backfill, `mission_skill_query` must operate on FTS alone. Already implied by the v3 blueprint phrasing *"FTS/vector ranking"* — keep FTS as the always-on path; vector is best-effort.

---

## 7. Skill-to-Workflow Distillation Plan

### 7.1 What can become a workflow

**Distillation criterion**: a section is workflow-shaped if it has (a) a deterministic input set, (b) ordered steps, (c) named tool calls, (d) explicit completion criteria. Sections that are **decision trees** (e.g. troubleshooting tables) stay in Markdown — they are reference, not procedure.

| Skill | Distillable section | Suggested workflow id | Mapping |
|---|---|---|---|
| `pcea` | `# deploy` (lines 70-171) | `pcea-deploy` | entry: `mission_swarm_run` with `target_branch`; steps mirror `git push → GA → DC → ECS` chain; completion = `m6-deployment-rollout`'s `c1` for slug `pcea-video-vault` AND `pcea-api` |
| `pcea` | `# bot-api` § "操作手册" (lines 472-590) | `pcea-bot-runbook` | entry: Bot JWT mint; steps for skill/list/batch-update/batch-transcribe/overview |
| `backend-deploy` | `# trigger` + `# verify` + `# workflows` (lines 91-113, 137-145, 326-385) | `backend-deploy-rollout` | already half-Lispified at lines 332-385; lift verbatim |
| `backend-deploy` | `# new-repo` (lines 257-282) | `register-new-repo-deploy` | exact-shard already (`xjp_deploy_project_create_from_template` + `xjp_agent_project_create`) |
| `deploy-ops` | `# 健康巡检` (lines 36-52) | `deploy-ops-checkup` | callable-on-cron version of `xjp_deploy_status` + `xjp_agent_health` + `xjp_deploy_pipeline_status` + `xjp_github_workflow_status` |
| `deploy-ops` | `# 配置漂移排查清单` (lines 142-177) | `config-drift-grep` (defensive) | turn the bash grep loop into a workflow with `agent_url` parameter |
| `deployment-troubleshoot` | `# chain-status` (lines 25-32) | `deploy-chain-status` | tiny wrapper over `xjp_update_chain_status` — almost overkill, but makes it Lisp-discoverable |
| `xjp-deploy-agent` | `# new-project` (lines 270-339) | `register-new-agent-project` | exact-shard for adding a `PROJECT_<NAME>_*` env block |
| `sqlx-cache` | (entire procedure) | `sqlx-cache-rebuild` | already a chain |
| `xjpfs` | `# troubleshoot` recovery loop | `xjpfs-restart-and-verify` | high recurrence in CI failures |

### 7.2 What should stay in Markdown

* **Decision tables**: `pcea#troubleshoot` (line 793-806), `deploy-ops#常见问题速查` (110-122), `deployment-troubleshoot#常见问题`. These are 1-D tables of `symptom → root cause → fix`, not procedures. Workflow form would be 30 dead branches. Keep as reference; let `mission_skill_context(section="troubleshoot")` return them on demand.
* **Reference sections**: env tables, port tables, runner-label tables. Pure facts, no procedure.
* **Architecture/explanation prose**: `# architecture`, `# pipeline`. Background reading.

### 7.3 Anti-pattern: pasting whole SKILL.md into agent prompt

The current `mission_task_delegate` policy (blueprint line 615) already forbids auto-prepending KB/Skill into worker prompts. Distillation respects that:

1. **Workflow body holds the procedure** (entry / steps / egress / risk-gates). The agent gets enough to act.
2. **Skill body is fetched on demand** via `mission_skill_context(skill_id, section=...)` at the precise step that needs it. Worker prompt embeds *only the section it needs*, not the whole 800-line file.
3. **Frontmatter alone is sometimes enough**: name + description + triggers + requires gives the agent a 5-line summary of the skill without any chunks loaded.

Concretely for `pcea-deploy` workflow step `s3 (build-and-trigger)`: the worker prompt would include only the OSS path table + the `Trigger Deploy Center (OIDC)` shell snippet — ~30 lines, not 800. This is the analogue of context-pack `read_scope` for skill bodies.

### 7.4 Distillation source-of-truth

Each distilled workflow declares `:source_skill <skill_id> :source_skill_sha256 <sha>`. Re-distillation is automatic on skill body change (`mission_skill.reconcile` enqueues a `skill-distill-review` BoardTask if the source skill body_sha256 has drifted past the workflow's pinned sha). This prevents silent drift between the workflow's procedure and the skill's narrative.

---

## 8. Recommended Next Shards (exact, ClaudeCode-ready)

Each shard is an *exact-shard* in the project-m6-depth sense: file or region ownership, no overlap, design pre-accepted.

### Shard A — Skill registry SSOT Lisp + checker

* **Write scope**: `.missiond/v3/policies/skill-registry-policy.lisp` (new); checker `scripts/check-skill-registry-isomorphism.mjs` (new); register both in `.missiond/v3/missiond-blueprint.lisp` skill-runtime section.
* **Body**: `skill-identity-contract` per §5.2; reconcile-action declaration; authority boundary (§5.6); FTS/vector field set per §6.2.
* **Deliverable**: Lisp shard + checker that fails CI when skill registry schema drifts from declared contract.
* **Acceptance**: `node scripts/check-skill-registry-isomorphism.mjs` exits 0 with no source code reading skill state.

### Shard B — PG migration + skill_chunks/embedding columns

* **Write scope**: `crates/missiond-daemon/migrations/<next>_skills.sql` only.
* **Body**: tables `skills`, `skill_chunks`, `project_skill_link` per §5.3; `body_tsv` GENERATED column with `to_tsvector('simple', name||' '||description||' '||body)` weighted; `pgvector` column `embedding vector(1024)`; HNSW index.
* **Acceptance**: `cargo sqlx prepare` clean; existing `skill_*` Rust handlers compile against the new tables (read-only at this stage).

### Shard C — Filesystem walker + frontmatter parser (no embedding yet)

* **Write scope**: `crates/missiond-daemon/src/handlers/knowledge/skill/reconcile.rs` (new module under existing `skill/` surface from blueprint line 3239); helper `crates/missiond-daemon/src/handlers/knowledge/skill/parser.rs`.
* **Body**: walk three skill roots + `services/*/.missiond/intent.lisp`; parse YAML frontmatter (`serde_yaml`); chunk by `^# `; compute `sha256`; UPSERT into `skills` + `skill_chunks` (embedding NULL); emit drift items per §5.5.
* **Acceptance**: `mission_skill.reconcile()` once produces non-zero `skills` rows, second call is idempotent (no UPDATEs unless mtime/sha changed).

### Shard D — Project-skill linker (joins to project-identity-contract)

* **Write scope**: extend `crates/missiond-daemon/src/handlers/knowledge/skill/reconcile.rs` (Shard C); add `mission_project.reconcile` integration in `services/project/registry.rs` (per blueprint line 3074).
* **Body**: from `skills.aka[]` + `triggers[]` + `requires.kb[]` derive candidate `project_skill_link` rows; resolve against `project-identity-contract.aliases[]` and `service_ids[]`; emit `Decision Inbox` item when ambiguous (e.g. PCEA one-vs-two project decision in §5.4).
* **Acceptance**: After reconcile, `SELECT count(*) FROM project_skill_link WHERE skill_id='pcea'` ≥ 1; ambiguity surfaces as `mission_question_create` decision.

### Shard E — `mission_skill_query` FTS path (vector path stub)

* **Write scope**: `crates/missiond-daemon/src/handlers/knowledge/skill/query.rs` (already exists per blueprint line 3239 — extend, don't replace).
* **Body**: implement RRF with FTS only (vector returns empty until Shard F lands); accept `project_id?`, `role?`, `limit`, `section?` filters; return `[{skill_id, section_anchor, snippet, score}]`.
* **Acceptance**: `mission_skill_query(query="deploy pcea-api")` returns `pcea#deploy` chunk in top-3.

### Shard F — Embedding worker for skill chunks

* **Write scope**: extend `crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs` (already exists per blueprint line 3149) to consume a new `ProcessSkillTopic` message kind; *do not* introduce a second worker.
* **Body**: Sonnet-gated topic extraction → POST `OLLAMA_URL/api/embed` via Router (same auth as PCEA); Matryoshka truncate to 1024; UPDATE `skill_chunks SET embedding = $1, updated_at = now()`.
* **Acceptance**: After Shard B+C+F, `SELECT count(*) FROM skill_chunks WHERE embedding IS NOT NULL` matches non-error chunks.

### Shard G — Vector-path completion in `mission_skill_query`

* **Write scope**: `skill/query.rs` again (extends Shard E).
* **Body**: vector path (`cosine_distance` on the new HNSW index); RRF merges FTS + vector; reserve no-op reranker call site.
* **Acceptance**: same query as Shard E returns same top-3, but with `score_components: {fts, vector, rrf}`.

### Shard H — Distill `pcea#deploy` into workflow

* **Write scope**: `.missiond/workflows/pcea-deploy.lisp` (new); does NOT modify any other workflow.
* **Body**: per §7.1 row 1; `:source_skill pcea`, `:source_skill_section "deploy"`, `:source_skill_sha256 <hash>`; entry calls `xjp_deploy_ci_trigger(project=...)` for the right slug; completion gated on `m6-deployment-rollout`'s `deploy_succeeded` event.
* **Acceptance**: `mission_swarm_run` with objective "deploy pcea front-end" matches this workflow via match-rule, and the worker prompt includes only the workflow body + `mission_skill_context(skill_id="pcea", section="deploy")`, not the whole 860-line skill.

### Shard I — Replace `DEPLOYMENT_MAP` hard-coding in M6 checker

* **Write scope**: `scripts/check-m6-deployment-status.mjs` only (already in this repo's scripts). Replace the const `DEPLOYMENT_MAP` block (lines 22-68) with a call to `mission_project(action=universe)` + `project_skill_link` join via a new `mission_project(action=deploy_components)` accessor (Shard J would add the accessor — but for a single-script change, fall back to reading `~/.claude/skills/<id>/SKILL.md` frontmatter `aka`).
* **Body**: walk `M6` projects from blueprint, look up `project_skill_link.role='primary'`, read deploy_slug from skill frontmatter `aka` matched against `project-identity-contract.deploy_center_slug`. Same classification logic.
* **Acceptance**: `node scripts/check-m6-deployment-status.mjs --json` returns identical `projects[]` shape as today, but adding a new M6 project no longer requires editing the JS map.

### Shard J — `mission_project(action=deploy_components)` accessor

* **Write scope**: `crates/missiond-daemon/src/handlers/services/project/universe.rs` (per blueprint line 3074) extension; new MCP tool variant in `intent-mcp-defs.lisp`.
* **Body**: returns `[{project_id, deploy_slug, repo_root, m6_relevant_paths}]` derived from `project_skill_link` + `project-identity-contract`.
* **Acceptance**: Shard I no longer needs JS-side fallback.

### Shard K — Service-manifest reader (deferred, depends on user authorization)

* **Write scope**: deploy-center side; out of scope for MissionD-only changes.
* **Recommendation**: defer this until DC is ready. Mention here only because `pcea-api/service.manifest.toml` is the natural next contract for deep-healthcheck/smoke automation. **Do not bundle into the Skill Registry shard ladder.**

---

## Verification

* No source code modified. Only the report file
  `/Users/jinchen/Projects/missiond/.missiond/research/pcea-deploy-skill-investigation-20260507.md`
  was written (write_scope honored).
* All claims map to a file path in the read_scope.
* No KB/history/provider logs were read. No `cargo fmt` invoked.
* `must_not_touch` honored: no writes to `crates/`, `packages/`, `scripts/`,
  PCEA repos, deploy-center repo, or other workflows.

### Files read (with confidence)

| Path | Confidence | Used for |
|---|---|---|
| `~/.claude/skills/pcea/SKILL.md` (full) | A | §1, §2, §4, §7 |
| `~/.claude/skills/pcea-knowledge/SKILL.md` (full) | A | §1, §6 |
| `~/.claude/skills/deployment/SKILL.md` (full) | A | §1, §3 |
| `~/.claude/skills/deploy-ops/SKILL.md` (full) | A | §1, §3, §7 |
| `~/.claude/skills/deployment-troubleshoot/SKILL.md` (full) | A | §1, §3, §7 |
| `~/.claude/skills/backend-deploy/skill.md` (full) | A | §1, §3, §7 |
| `~/.claude/skills/xjp-deploy-center/skill.md` (full) | A | §1, §3 |
| `~/.claude/skills/xjp-deploy-agent/SKILL.md` (full) | A | §1, §3 |
| `~/.codex/skills/` (listing only) | A | §1.D negative finding |
| `~/.agents/skills/` (listing only) | A | §1.D negative finding |
| `~/Downloads/PCEA develop/pcea-api/.github/workflows/deploy.yml` | A | §2.1, §2.3 |
| `~/Downloads/PCEA develop/pcea-video-vault/.github/workflows/deploy.yml` | A | §2.1, §2.3 |
| `~/Downloads/PCEA develop/pcea-video-vault/deploy.sh` | A | §2.1, §2.3 |
| `~/Downloads/PCEA develop/pcea-video-vault/deploy-api.sh` | A | §2.1 |
| `~/Downloads/PCEA develop/pcea-video-vault/docker-compose.yml` | A | §2.1 |
| `~/Downloads/PCEA develop/pcea-api/service.manifest.toml` | A | §2.1, §3.3, §4-G3 |
| `~/Downloads/PCEA develop/pcea-api/` (file listing, Cargo.toml/Dockerfile names only) | B | §2.1 |
| `~/Downloads/PCEA develop/pcea-video-vault/` (file listing) | B | §2.1 |
| `services/deploy-center/src/api/status.rs` (lines 1-240) | A | §3.1, §3.4 |
| `services/deploy-center/src/api/` (dir listing) | A | §3.1 |
| `services/deploy-center/migrations/` (listing + 0041 head 30 lines) | A | §3.1, §3.2 |
| `apps/xjp-deploy-agent/` (dir listing only) | B | §3.2 — no source files inside |
| `.missiond/workflows/m6-deployment-rollout.lisp` (full) | A | §3.4, §4-G2, §7 |
| `.missiond/workflows/multi-project-m6-wave.lisp` (full) | A | §7 (workflow-shape reference) |
| `.missiond/workflows/project-m6-depth.lisp` (head) | A | §8 (exact-shard discipline) |
| `.missiond/workflows/project-registry-reconciliation.lisp` (full) | A | §3.1, §5.4 |
| `.missiond/workflows/deployment-event-response.lisp` (full) | A | §3.1 |
| `.missiond/v3/missiond-blueprint.lisp` (greps for skill / fts / embedding / project-identity, line 1163-1206 + 1471 + 1914 + 1957-1962 + 2066-2071 + 2353 + 2420-2425 + 3074 + 3115-3137 + 3149 + 3167 + 3235-3245 + 615-616 + 824) | A | §5.1, §5.4, §5.6, §6 |
| `scripts/check-m6-deployment-status.mjs` (full) | A | §3.4, §4-G2, §8-Shard I |

### Coverage gaps (intentional, declared)

* `apps/xjp-deploy-agent/{src,crates,bins,migrations}/**` not opened — relied on `xjp-deploy-agent/SKILL.md` (skill content matches the directory listing observed). Shard B/C may need a deeper read at execution time.
* `services/deploy-center/src/api/{projects,trigger,webhook}.rs` not opened — claims about `/ci/trigger/` semantics come from `backend-deploy` skill + GA workflow body. Adequate for design; not adequate for implementation.
* No KB/history/provider logs read (per task constraint).
