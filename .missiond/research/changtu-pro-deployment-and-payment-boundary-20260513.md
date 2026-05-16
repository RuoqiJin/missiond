# changtu.pro deployment and payment boundary

Date: 2026-05-13

## Purpose

Record the deployment design for the standalone Long Image Service and the current Aliyun DNS capability status so MissionD workers do not need to rediscover the PCEA deployment chain from raw skills every time.

## Deployment Shape

The CN deployment should follow the proven PCEA artifact-bundle route:

```text
GitHub repo
  -> privatecloud GitHub Actions runner
  -> Docker image + deploy scripts + compose bundle
  -> Aliyun OSS bucket rickyjim
  -> Deploy Center project long-image-service-cn
  -> Aliyun ECS deploy-agent project changtu
  -> /opt/changtu
  -> changtu.pro
```

Key inherited PCEA rules:

- Do not build on ECS.
- Do not rely on ECS GitHub/GHCR/Docker Hub access.
- Use OSS internal endpoint from ECS.
- Bundle must carry deployment scripts and compose files so the ECS side is self-updating.
- Deploy Center remains the deployment fact authority.

## Runtime Target

| Field | Value |
| --- | --- |
| Service | `long-image-service` |
| CN domain | `changtu.pro` |
| Runtime | Aliyun ECS, Shanghai |
| ECS public IP | `106.15.2.17` |
| App dir | `/opt/changtu` |
| Container port | `4177` |
| Deploy Center project | `long-image-service-cn` |
| Deploy agent project | `changtu` |

## DNS Capability

The local Aliyun CLI exists, but current DNS API credentials are not usable:

```text
aliyun alidns DescribeDomainRecords --DomainName changtu.pro
-> InvalidAccessKeyId.NotFound
```

MissionD should treat Aliyun DNS mutation for `changtu.pro` as `blocked: credential-invalid` until a valid credential is bound through secret-store/deploy-center.

Expected DNS records after credentials are fixed:

| Host | Type | Value |
| --- | --- | --- |
| `@` | `A` | `106.15.2.17` |
| `www` | `A` | `106.15.2.17` |

If the global Vercel version needs a public hostname, use `global.changtu.pro` or another explicit subdomain rather than mixing CN/global ledgers behind one ambiguous hostname.

## Payment Boundary

Long Image membership should use the XJP Payments model, but region ledgers must be split:

- CN: CN XJP Payments, WeChat/Alipay provider set, CN webhook secrets, CN order/refund/webhook/idempotency history.
- Global: global XJP Payments, Stripe or global provider set, global webhook secrets, global order/refund/webhook/idempotency history.

Auth owns tenant/application/product/user-group identity context. Payments owns order history, refunds, provider webhooks, idempotency, credit/membership fulfillment, and invoices.

Proposed first SKU:

```text
tenant: long-image
application: changtu
product: long-image-membership
sku: long_image_membership_monthly
price: 9.9 CNY
```

## Follow-Up Work

- CN deploy workflow and deploy scripts are now present in `long-image-service`.
- `long-image-service-cn` is registered in Deploy Center.
- Aliyun DNS credential has been replaced and read access was verified through the Aliyun CLI; mutating DNS remains an approved deploy-ops action.
- Deploy Center runtime target/provenance for `changtu.pro` is now partially recorded and should continue to be reconciled from Deploy Center, not from ad hoc curl/git evidence.
- Add CN/global payment service registry entries before enabling paid membership.

## Deployment Experience Captured 2026-05-16

This deployment produced a reusable deploy-ops lesson for MissionD:

- Direct ECS agent execution can prove that an on-host script works, but it does not prove that the Deploy Center project is wired correctly.
- The formal Deploy Center path for a CN service is owned by `deploy_project_stage_configs`: stage, executor, executor project, work dir, and script must all point at the intended runtime target.
- `long-image-service-cn` initially failed through Deploy Center because the stage config was still bound to `privatecloud-agent`; direct `changtu` execution on ECS succeeded, so the root cause was not the application image or `/opt/changtu/deploy.sh`.
- Updating the stage config to executor `ecs-agent`, executor project `changtu`, work dir `/opt/changtu`, script `./deploy.sh` produced Deploy Center deployment `ea63e65c-b1f2-4952-a06f-25aed131a9e3`, claimed by agent `ecs`, with reported digest `sha256:119764945be606ae0eb883f342d5beefe0ab279c28ba460c31281df4c15ac02c`.
- Internal ECS and Caddy host-header health passed for `long-image-service`; public `changtu.pro` remains blocked by Aliyun ICP filing before the request reaches Caddy.

Workflow implication:

- A deployment worker should verify both the direct runtime health and the Deploy Center formal path.
- If direct runtime succeeds but Deploy Center fails, inspect stage config/executor mapping before changing application code.
- If host-header health succeeds but public domain returns the Aliyun ICP interstitial, classify it as domain filing/gateway status, not app health failure.
