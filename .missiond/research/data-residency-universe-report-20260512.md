# Data Residency Universe Research Summary

Date: 2026-05-12

Scope: PCEA as the first data-bearing M6 project, CUTHUB as the next public application case.

## Decision

MissionD should model data residency as a first-class `data-residency-universe` surface. The active partition model is:

- `cn` and `global` are hard partitions.
- `global-eu` is an operating zone inside `global`, not a separate hard partition by default.
- PCEA-CN and PCEA-GLOBAL must use separate issuers, signing keys, KEKs, storage, payment ledgers, model-router policy, event topics, and deploy targets.
- Cross-region data movement is default-deny and only allowed through named egress categories.
- CUTHUB should use separate domains and accounts: `cuthub.cn` for mainland China and `cuthub.com` for global users. Parent-domain cookie sharing and IP-only region binding are forbidden.

## Rationale

The hard-partition boundary follows the AWS partition model: credentials and trust do not cross partitions. EU is treated more like an operating zone inside the global partition, following EU data-boundary / Hyperforce-style data residency patterns.

The China mainland path requires conservative defaults because personal data, sensitive personal information, payment ledgers, generated AI data, and possible important-data classification can trigger separate legal, runtime, and payment obligations.

## Primary Sources

- CAC, 2024-03-22, `促进和规范数据跨境流动规定`: https://www.cac.gov.cn/2024-03/22/c_1712776611775634.htm
- AWS fault isolation boundary / partitions: https://docs.aws.amazon.com/whitepapers/latest/aws-fault-isolation-boundaries/partitions.html
- Salesforce Data 360 security architecture / operating-zone data residency concepts: https://architect.salesforce.com/docs/architect/fundamentals/guide/data360_security_architecture

## SSOT Landing

- MissionD V3: `.missiond/v3/missiond-blueprint.lisp` `(data-residency-universe ...)`
- PCEA project SSOT: `/Users/jinchen/Downloads/PCEA develop/.missiond/intent.lisp` `:data-residency`
- Checker: `scripts/check-v3-data-residency-universe-isomorphism.mjs`

## Runtime Placement Follow-up

Date: 2026-05-13

User decision: PCEA `global` runs on the GCP VM lane; PCEA `cn` runs on Aliyun ECS.

Current verified CN lane:

- Deploy Center projects: `pcea-video-vault`, `pcea-api`.
- Runtime target: `ecs-pcea` through the `ecs` deploy-agent.
- Public domain: `pcea.top`.
- Frontend deployed commit: `b414107a6e3a7fbe5b7460b6987dfd65591acdff`.
- API deployed commit: `9c0b941099e28b94e315a7a4c4599d912e0d2fcb`.
- Deploy style: GitHub Actions/privatecloud build, artifact bundle in Aliyun OSS, Deploy Center dispatch, ECS agent executes `/opt/pcea/deploy.sh` or `/opt/pcea/deploy-api.sh`.
- Invariant: target-side builds are forbidden; deployment scripts consume immutable artifacts.

Global lane status:

- Intended runtime target: `gcp-runtime` through the GCP deploy-agent.
- Public domain target: `pcea.io`.
- Status: target pending provisioning.
- Missing deploy-center facts: separate global project slugs, per-lane release provenance, secret-store namespace, auth issuer, storage/payment ledgers, smoke report, rollback artifact.
- Current CN deploy scripts cannot be copied blindly to GCP because they hardcode CN deployment assumptions: `oss-internal`, `pcea.top`, `/opt/pcea`, local Postgres/OSS configuration, and ECS-side `mc`/docker-compose setup.

Gate behavior:

- `check-m6-deployment-status.mjs --project pcea` must report `regional-rollout-incomplete` until the GCP lane has its own deploy-center project/provenance and smoke evidence.
- Existing CN deployment can be considered current only for `pcea-cn`, not for `pcea-global`.

## Platform Partition Follow-up

Date: 2026-05-13

Refined decision: this is not primarily a PCEA split. It is an XJP platform split:

- `xjp-cn`: China mainland infrastructure partition on the Aliyun ECS lane. It owns the CN-side auth, secret-store, payment ledger, storage ledger, router policy, eventhub/timeline, deploy-center lane, and deploy-agent provenance.
- `xjp-global`: overseas infrastructure partition on the GCP VM lane. It owns the global auth, secret-store, payment ledger, storage ledger, router policy, eventhub/timeline, deploy-center lane, and deploy-agent provenance.
- `xjp-global-eu`: EU operating zone inside `xjp-global` until EU storage/KMS/support-access pinning and dedicated provenance justify promoting it to a separate hard partition.

Applications bind to platform partitions:

- `pcea-cn` binds to `xjp-cn`.
- `pcea-global` binds to `xjp-global`.
- `pcea-global-eu` binds to `xjp-global-eu`.
- `cuthub-cn` and `cuthub-global` should follow the same binding pattern once CUTHUB reaches its data-bearing M6 wave.

The practical implication is that adding the next app should not require another bespoke data-residency design. The app declares its data classes and platform binding; XJP platform partitions provide the runtime, auth, secret, payment, storage, model-routing, event, deployment, and observability boundaries.
