# M6 Maturity Audit: App-Layer SSOT Batch

- BoardTask: `7133cb0b-ee3a-494b-9f3c-4318c8525536`
- Date: 2026-05-05
- Scope: `pcea`, `xiaojin-blog`, `cuthub`
- Mode: static-only. Evidence came from the named project roots, target-local `.missiond` intent/blueprint/evidence files, target-local checker scripts, and the MissionD project-universe checker. No KB, provider logs, Board backlog, production probes, Cloudflare, Kubernetes, live ops, package installs, or target source edits were used.

## Verdict

All three targets classify as **M6** for the app-layer SSOT contract. No real M6 blockers were found, so no child BoardTasks are required.

| Target | Root | Classification | Rationale |
|---|---|---:|---|
| `pcea` | `/Users/jinchen/Downloads/PCEA develop` | M6 | Root L1 intent exists; backend and frontend blueprints exist; every audited function has `:entry`, `:core`, `:egress`, `:surfaces`, and `:runtime-projection`; local checker passes; M6 convergence evidence exists. |
| `xiaojin-blog` | `/Users/jinchen/Projects/xiaojin-blog` | M6 | L1 intent, backend blueprint, frontend blueprint, evidence report, and static checker are present; checker gates root/repo/package/framework/pillars/routes; universe checker locates the project and reports green. |
| `cuthub` | `/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/cuthub-frontend` | M6 | Canonical-temporary L1 intent, frontend blueprint, evidence report, and static checker are present; checker gates root annotation/repo/package/framework/pillars/upstream services/routes; universe checker reports green. |

## Evidence

### pcea

- Root intent: `.missiond/intent.lisp`
  - Declares root `/Users/jinchen/Downloads/PCEA develop`, canonical backend `pcea-api`, canonical frontend `pcea-video-vault`, and quality gates.
- Backend blueprint: `.missiond/backend/pcea-backend-blueprint.lisp`
  - Functions cover router, principal resolver, worker embedding loop, COS/S3 storage, LLM bridge, and deep health.
  - Runtime projection includes database, auth, storage, payments, LLM, Ollama, deploy-center, and related env anchors.
- Frontend blueprint: `.missiond/frontend/pcea-frontend-blueprint.lisp`
  - Functions cover app shell, routing, API client, auth token/PKCE flow, and admin/video-vault surfaces.
  - Runtime projection is `[VITE_API_URL VITE_AUTH_BASE VITE_SITE_URL]`.
- Evidence report: `.missiond/evidence/m6-convergence-report.md`
  - Records current-code mapping and deferred non-blocking build/test decisions.
- Checker: `node scripts/check-pcea-ssot-complete.mjs --json`
  - Result: `{"ok": true, "diagnostics": []}`.
- Known build/test gates from SSOT: `cargo test --manifest-path pcea-api/Cargo.toml`, `npm --prefix pcea-video-vault run build`; not run in this audit to preserve static-only mode.

### xiaojin-blog

- Root intent: `.missiond/intent.lisp`
  - L1 index for Next.js app, auth, pages, API routes, data layer, deployment, and roadmap.
- Backend blueprint: `.missiond/backend/xiaojin-blog-backend-blueprint.lisp`
  - Covers API routes, Drizzle/Postgres data layer, and auth issuer integration.
  - Functions carry entry/core/egress/surfaces/runtime-projection coverage.
- Frontend blueprint: `.missiond/frontend/xiaojin-blog-frontend-blueprint.lisp`
  - Covers identity, pages, auth UI, globe, admin/editor/drafts, and shell surfaces.
- Evidence report: `.missiond/evidence/m6-convergence-report.md`
  - Records M4/M5/M6 transition, current-code mapping, dirty-baseline handling, and known build/lint commands.
- Checker: `bash .missiond/check.sh`
  - Passed all 11 gates: root, repo, package, Next 16, React 19, package manager, Next config, 7 pillars, 13 route anchors, diff-check.
- Known build/test gates from SSOT: `pnpm build`, `pnpm lint`, `pnpm tsc --noEmit`; not run in this audit to preserve static-only mode.

### cuthub

- Root intent: `.missiond/intent.lisp`
  - L3 implementation index with canonical temporary root, design constraints, upstream services, 11 pillars, routes, and key flows.
- Frontend blueprint: `.missiond/frontend/cuthub-frontend-blueprint.lisp`
  - Covers all 11 intent pillars through 8 M6 functions: infrastructure, auth, project/timeline, FCPXML, Shot Lab, subtitle/review, media/profile, and marketing.
  - Functions carry entry/core/egress/surfaces/runtime-projection coverage.
- Evidence report: `.missiond/evidence/m6-convergence-report.md`
  - Records current-code mapping, canonical-temporary annotation, dirty-baseline handling, and known build/lint commands.
- Checker: `bash .missiond/check.sh`
  - Passed all 13 gates: root, canonical-temporary annotation, repo, package, Next 16, React 19, npm lockfile, CSP config, 11 pillars, 6 upstream services, 10 route anchors, diff-check.
- Known build/test gates from SSOT: `npm run build`, `npm run lint`, `npx tsc --noEmit`; not run in this audit to preserve static-only mode.

## Universe Checker

Command:

```bash
cd /Users/jinchen/Projects/missiond
node scripts/check-project-ssot-universe.mjs --json
```

Result: `ok: true`, `diagnostics: []`.

Relevant target entries:

- `pcea`: `ok: true`, command `node scripts/check-pcea-ssot-complete.mjs --json`
- `xiaojin-blog`: `ok: true`, command `bash .missiond/check.sh`
- `cuthub`: `ok: true`, command `bash .missiond/check.sh`

## Non-Blocking Deferred Items

These are already recorded by target-local evidence and do not block M6 classification:

- `pcea`: top-level `.missiond/` tier is outside the nested `pcea-api` / `pcea-video-vault` git repos; frontend root quality gate mentions `pnpm` while the frontend has npm lockfile evidence.
- `xiaojin-blog`: `/drafts/:path*` is protected at runtime but not listed in the intent matcher; Cesium CDN runtime version differs from the package version; PgBouncer and git-backed post visibility are deferred.
- `cuthub`: canonical root is explicitly temporary under `Downloads`; `/fcpxml/mobile` has not migrated to `FcpxmlEditor`; `lib/srsApi.ts` appears unused; `presign-batch` is declared conceptually but folded into the `presign` handler.

## Close-Out

No child BoardTasks were created because no target failed the M6 contract. Parent BoardTask can be marked done after this artifact and the Board note are durable.
