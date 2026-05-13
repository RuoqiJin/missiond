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

- Add the CN deploy workflow and deploy scripts to `long-image-service`.
- Register `long-image-service-cn` in deploy-center.
- Bind valid Aliyun DNS credential through secret-store/deploy-center.
- Add Deploy Center runtime target/provenance for `changtu.pro`.
- Add CN/global payment service registry entries before enabling paid membership.
