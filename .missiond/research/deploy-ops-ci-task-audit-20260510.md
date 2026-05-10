# Deploy / Ops / CI Board Audit — 2026-05-10

This is a read-only MissionD audit of open Board tasks matching deployment, ops,
CI, agent, runner, GCP/ECS/VM, DNS, and deploy-center keywords.

## Sources Checked

- MissionD Board open tasks from the local `missiond` database.
- MissionD V3 SSOT:
  - `.missiond/v3/missiond-blueprint.lisp`
  - `.missiond/workflows/m6-deployment-rollout.lisp`
  - `.missiond/workflows/pcea-deployment-rollout.lisp`
  - `.missiond/workflows/deployment-event-response.lisp`
  - `.missiond/workflows/project-registry-reconciliation.lisp`
- ClaudeCode deployment skills:
  - `/Users/jinchen/.claude/skills/deploy-ops/SKILL.md`
  - `/Users/jinchen/.claude/skills/xjp-deploy-center/skill.md`
  - `/Users/jinchen/.claude/skills/xjp-deploy-agent/SKILL.md`
  - `/Users/jinchen/.claude/skills/private-cloud/SKILL.md`
  - `/Users/jinchen/.claude/skills/windows-runner/SKILL.md`
  - `/Users/jinchen/.claude/skills/pcea/SKILL.md`
- Deployment checks:
  - `node scripts/check-m6-deployment-status.mjs --json`
  - `node scripts/check-project-ssot-universe.mjs --json`
  - `node scripts/check-v3-final-convergence.mjs --json --static-only`

Secrets observed in skill files were not copied here.

## Current Deployment Fact Snapshot

`scripts/check-m6-deployment-status.mjs --json` reports the current M6 deployment
targets as healthy and current:

| Project | Deployment Status | Deploy Center Slug(s) | Remaining Fact Gap |
|---|---|---|---|
| deploy-center | deployed-current | `xjp-deploy-center` | provenance is partial because `reported_digest` is missing |
| auth | deployed-current | `xjp-auth-center` | provenance is partial because `reported_digest` is missing |
| router | deployed-current | `xjp-router` | provenance is partial because `reported_digest` is missing |
| pcea | deployed-current | `pcea-api`, `pcea-video-vault` | provenance is partial: digest and workflow-run rows missing for PCEA |
| deploy-agent | no-deploy-target | none | handled by deploy-agent self-update provenance, not ordinary service deploy |

This means deployment work should not begin from ad-hoc `git`, `curl`, or GitHub
status reconstruction. Deploy-center status/provenance is the authority; curl and
GitHub are diagnostics.

## SSOT Coverage Already Present

MissionD V3 already contains the correct high-level architecture:

- `eventbridge-deployment-plane`: deployment events enter MissionD as typed
  external service events.
- `m6-deployment-confirmation`: M6 maturity is not deployment evidence; release
  closure requires deploy-center provenance plus smoke evidence.
- `registry-authority-map`: MissionD owns project identity/SSOT/maturity;
  deploy-center owns deployment target/runtime/provenance/agent executor state;
  Forge owns component/pattern catalog.
- `skill-runtime`: operational skill facts such as 12900KF, Windows runner,
  deploy-agent, router embedding/rerank, and remote host facts are retrievable
  as skill-derived operational facts before ad-hoc probing.
- `pcea-deployment-rollout`: PCEA deployment must use skill context as evidence
  refs and deploy-center provenance/events as closure authority.

## Board Task Classification

The Board currently has about 137 visible non-closed tasks matching deployment /
ops / CI keywords. They should not all be run as active deployments.

### Already Resolved / Stale Wrapper

- `5a1554fc-511a-4aec-87cc-461e8d7d70cf`
- `a086b1e8-5e8a-42dd-9245-abd39ee0aad2`

Both PCEA deployment skill investigation artifacts already exist:

- `.missiond/research/pcea-deploy-skill-investigation-20260507.md`
- `.missiond/research/pcea-deploy-and-skill-management-investigation-20260507.md`

These two BoardTasks were marked `done` in this audit. The remaining reusable
gaps belong to skill-runtime, deploy workflow, and project registry convergence.

### MissionD / Workflow Fixes Still Actionable

- `882527ab-ba9a-4d9c-8c45-02501534aaf5`
  - Fix deploy workflow issues observed during Auth M6 deploy.
  - Still relevant as a workflow hardening parent: explicit deploy-ops lane,
    durable final evidence, structured acceptance smoke, no worker shell sleep
    as deployment monitor.
- `3de408bb-e847-47e4-8a9a-312c23b8c522`
  - `xjp_deploy_status()` global timeout.
  - Still relevant as deploy-center observability hardening, even though
    project-scoped status/provenance currently works.
- `8ef9a404-c830-482a-aa7a-4653f1171d9d`
  - Dormant `xjp-auth-center` `prefetch_image` drift.
  - Low-risk config correction, but it mutates deploy-center config and should
    go through a deploy-ops task, not ad-hoc SQL/curl.

### Needs Fresh Deploy-Ops Fact Check

These tasks mention live remote state or credentials. They should be handled by a
ClaudeCode deploy-ops lane using the deployment skills and deploy-center/XJP MCP
tools, then stored as deployment facts or closed as stale:

- `3989fc96-49d4-4c64-923d-67a518058ecf`: GCP agent API key failure.
- `d0077511-c738-424c-889e-70a6e42dc56e`: hostvds `xjp_agent_exec` timeout.
- `a775b2d7-56c3-4616-b5bc-c3962b4772b0`: Windows workstation health.
- `8eff0df1-16b4-4a07-b7a1-633171bc6942`: self-hosted runner stale busy.
- `5b2af4f8-4d17-45f0-8aad-be34bac06c64`: deploy-agent intermittent 502.
- `d2bc785a-0311-4286-a998-c28fa566943c`: Windows SSH relay / FRP.
- `a42d8b6c-328e-405b-9e4b-82eb11c0951b`: GCP snapd / apt lock.

### Historical Architecture Drift Candidates

Many open `架构漂移` deploy-center/deploy-agent/CI tasks appear to describe
architecture evolution that is now represented in the M6 SSOT:

- deploy-center trigger/executor separation
- pull-based agent task claiming
- push/event-driven agent update
- blue-green / zero-downtime update direction
- CI/CD responsibility split between GitHub Actions and deploy-center

These should be closed only after a batch audit maps each item to a specific
current SSOT function, checker, or code surface. They are not urgent live ops
incidents.

## Execution Recommendation

1. Keep deploy-center as deployment fact authority.
2. Use MissionD Board/Universe only for identity, SSOT, maturity, and workflow
   triggering.
3. For any unknown server or runner fact, first call `mission_skill_context`
   or equivalent skill-runtime lookup for `deploy-ops`, `xjp-deploy-agent`,
   `private-cloud`, `windows-runner`, or `pcea`.
4. If skill-derived facts are still insufficient, dispatch a read-only
   ClaudeCode deploy-ops investigation worker.
5. Store verified stable facts in deploy-center SSOT/provenance or MissionD
   Universe summaries; do not leave them only in prompt text or KB memories.
6. Close old Board tasks only after they are mapped to one of:
   - live issue still present
   - covered by SSOT/code/checker
   - stale historical evidence
   - needs user decision

## Next Exact Shards

1. `deploy-workflow-hardening`
   - Target BoardTask: `882527ab-ba9a-4d9c-8c45-02501534aaf5`
   - Goal: ensure deploy/ops tasks route through explicit deploy-ops lane and
     close only on deploy-center provenance plus smoke evidence.

2. `deploy-center-status-observability`
   - Target BoardTask: `3de408bb-e847-47e4-8a9a-312c23b8c522`
   - Goal: reproduce global `xjp_deploy_status()` timeout and add bounded
     parallel fanout or cached partial response if still present.

3. `deploy-center-prefetch-config-drift`
   - Target BoardTask: `8ef9a404-c830-482a-aa7a-4653f1171d9d`
   - Goal: update dormant `xjp-auth-center` prefetch image through deploy-center
     config API, then verify detail view.

4. `live-ops-fact-check-wave`
   - Targets: GCP agent auth, hostvds timeout, Windows runner, FRP, runner stale
     busy, deploy-agent 502.
   - Goal: ClaudeCode deploy-ops read-only investigation; write structured
     deployment facts and recommended closures.

