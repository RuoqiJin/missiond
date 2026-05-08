# Deploy-center rollout experience: 2026-05-08

## What happened

- Canonical XJP repo: `/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend`.
- Deployed service: `xjp-deploy-center`.
- Deployed commit: `bf4d2d275a0325c17a8f2382b31bbaeefa7bf48d`.
- GitHub Actions run: `25536497501`.
- Image tag: `ghcr.io/xiaojinpro-team/xjp-deploy-center:bf4d2d27`.
- Push log digest: `sha256:67be81d59a7f184a383ccc1090d8a7f661221e1a5d2d970dcb10d09b205f15f1`.
- Production health: `https://auth.xiaojinpro.com/api/deploy/health` returned `{"status":"ok","service":"deploy-center"}`.
- Deploy-center provenance confirmed the deployed commit, but confidence stayed `partial` because `reported_digest` was missing from the deploy log.

## Lessons

- GitHub Actions green and deploy-center notify HTTP 200 are progress evidence, not completion evidence.
- Deployment closure requires deploy-center provenance plus service smoke.
- `reported_digest_missing` is an evidence-quality gap in deploy-agent/deploy-center provenance, not necessarily a failed deploy.
- `sccache` and future cache layers are deployment speed aids. Missing cache infrastructure must fall back or produce a diagnostic, not block release.
- MissionD should ask deploy-center for release facts; curl, git, and GitHub are diagnostics unless deploy-center has no answer.

## Follow-up

- Push/deploy the deploy-agent provenance improvements when deploy-agent reaches M6.
- Keep `scripts/check-m6-deployment-status.mjs --json` as the operator-facing deployment status answer.
- Ensure deployment workflows emit typed diagnostics for `digest_resolution_failed`, `reported_digest_missing`, `runner_queued`, `build_cache_unavailable`, and `provenance_partial`.
