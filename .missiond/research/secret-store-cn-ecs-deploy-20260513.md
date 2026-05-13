# Secret Store CN ECS deployment preflight

Date: 2026-05-13

This is a redacted deployment preflight artifact. It contains no credential values.

## Findings

- Current Secret Store production endpoint is still `https://ss.xiaojinpro.top` on the GCP `xjp-backend` VM.
- The new Aliyun account credential is stored in Secret Store namespace `aliyun-global`.
- Aliyun OpenAPI read-only checks succeeded with the `aliyun-global` secret refs.
- The Aliyun account contains:
  - ECS instance `i-uf6641fl52xo7ukf7kgl`, instance name `iZuf6641fl52xo7ukf7kglZ`, status `Running`, public IP `106.15.2.17`, zone `cn-shanghai-e`.
  - Domains `changtu.pro`, `pcea.top`, and `xiaojinpro.com`.
- `changtu.pro` currently has zero DNS records in Aliyun DNS.
- Deploy Center shows the ECS deploy-agent online at version `10.7.2`.
- ECS deploy-agent current project list is `jinstudio`, `pcea`, `pceaapi`.
- ECS current container inventory is `pcea-app`, `pcea-api`, and `pcea-postgres`; no Secret Store container is running on ECS.
- Deploy Center project `secret-store` exists but has no deployment history and no usable agent execution configuration.
- Deploy Center CLI can formally manage this through `xjp project create/update` and `xjp project agent-exec-config-set`.
- Existing ECS-backed PCEA projects use `agent_config.deploy_type=execute_command`, `work_dir=/opt/pcea`, and scripts such as `./deploy.sh` / `./deploy-api.sh`.
- The current `secret-store` project points at repo `rickyjim626/secret-store-rs`, branch `main`, `deploy_type=rust-binary`, and `skip_deployment=true`; it is not an ECS deployment target.
- Deploy Center project `secret-store-cn` has now been created as a separate CN runtime shell:
  - repo `rickyjim626/secret-store-rs`, branch `main`
  - `deploy_type=docker_compose`
  - target host `ecs-agent`, target path `/opt/secret-store-cn`
  - `skip_deployment=true`
  - agent execution config points at `docker/docker-compose.stg.yml`
- Normalized Deploy Center stage configs for `secret-store-cn` now exist for both `build` and `deploy`, both bound to `ecs-agent` and both `enabled=false`.
- The disabled stage configs intentionally include `blocked_until` reasons: CN endpoint/domain decision, Postgres/Redis topology, `SECRET_STORE_MASTER_KEY_1` secret ref, `SECRET_STORE_ADMIN_KEY` secret ref, and production compose override.
- `xjp project info secret-store-cn` now surfaces normalized disabled stage configs instead of printing `(No agent configured)`. This fixed a CLI read-model observability gap: Deploy Center already had the stage config facts, but the CLI previously only displayed legacy agent-config shapes.
- `secret-store-rs` has deploy assets that a deploy-ops worker can evaluate before choosing a Deploy Center execution type:
  - `docker/docker-compose.stg.yml`
  - `docker/Dockerfile`
  - `deploy-to-gcp.sh`
  - `scripts/build-and-push-vm.sh`
  - `scripts/vm-setup-ghcr.sh`
  - `scripts/migrate.sh`
  - `scripts/smoke.sh`
  - `scripts/backup.sh`

## Decision

Do not mutate DNS or deploy a CN Secret Store container from this Codex thread yet.

The missing facts are load-bearing:

- CN Secret Store endpoint/domain: for example a dedicated CN host such as `ss-cn.xiaojinpro.com`, or a regional split strategy.
- Independent CN database and KEK/admin-key material.
- Deploy Center project/stage config for `secret-store-cn` exists and is bound to the ECS deploy-agent, but it is intentionally disabled until runtime material is ready.
- Health endpoint and rollback plan.
- Provenance row in Deploy Center after deployment.

## Infrastructure risk found

The `aliyun` skill previously contained inline SSH credential material. It has been changed to reference secret-store refs only. Future deploy-ops worker context must use redacted skill evidence and secret refs, never inline credentials.

The `secret-store-rs` repository also contained a historical inline GitHub container registry token in `scripts/vm-setup-ghcr.sh`. That script now requires `GITHUB_PAT` from the environment / secret-store injection, and the project-local checker now includes a `deployment-secret-hygiene` gate.

## Next shard

Promote the existing Deploy Center `secret-store-cn` project/runtime target only after CN endpoint, runtime material, and rollback decisions are declared. Do not apply Aliyun DNS writes yet.

## Candidate Deploy Center shape

The next worker should not invent an ad-hoc container deployment. It should first make Deploy Center authoritative for the CN runtime:

- project slug: `secret-store-cn`
- repo: `rickyjim626/secret-store-rs`
- branch: `main`
- executor: ECS deploy-agent
- deploy type: `execute_command` or `docker_compose`, chosen after reading `secret-store-rs` deploy assets
- work dir: a dedicated path such as `/opt/secret-store-cn`
- endpoint: undecided; candidate should be declared before DNS writes
- DB/KEK/admin material: secret refs only, independent from global/GCP runtime unless a deliberate shared-backend decision is made
- rollback: previous container image/config + DNS rollback manifest

Blocked facts that still need a decision or a deploy-ops investigation:

1. CN endpoint naming strategy.
2. Whether CN Secret Store uses a new Postgres DB/container on ECS or an existing managed Postgres target.
3. Which secret refs hold CN `KEK`, admin key, DB URL, and deploy-agent credentials.
4. Whether the first deploy should be canary-only on an internal hostname before public DNS.
5. Whether `docker/docker-compose.stg.yml` should be replaced by a production CN compose override before enabling Deploy Center stages.
6. Deploy Center UI/API should keep displaying disabled normalized stage configs as `configured-disabled`; the XJP CLI read model now does this for `xjp project info secret-store-cn`.
