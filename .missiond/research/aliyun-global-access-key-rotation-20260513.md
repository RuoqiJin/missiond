# Aliyun global AccessKey rotation — 2026-05-13

## Summary

The user deleted three old Aliyun AccessKey IDs and provided a new `AccessKey.csv` in `/Users/jinchen/Downloads/AccessKey.csv`. The new key is an Aliyun account-level credential, not a DNS-only credential.

## Canonical Secret Store Placement

- Namespace: `aliyun-global`
- Keys:
  - `ALIYUN_ACCESS_KEY_ID`
  - `ALIYUN_ACCESS_KEY_SECRET`
- Values are stored only in Secret Store. MissionD Lisp, Board notes, context packs, and worker prompts must reference `secret-store://aliyun-global/...` and must not include the key values.

A temporary narrower namespace, `aliyun-dns`, was created during initial diagnosis and then cleared after realizing the credential is global. Future DNS/OSS/ECS operations must reference the account-level namespace and declare the required capability in the target context.

## Verification

- Secret Store write: `aliyun-global` contains both expected keys with version `1`.
- Aliyun CLI read-only DNS probe: `alidns DescribeDomainRecords --DomainName changtu.pro` succeeded using one-shot CLI arguments and returned zero records.
- No key value was persisted to a repo or printed in logs; only a suffix was used for local verification.

## Operational Rule

Credential rotation, DNS mutation, OSS/ECS setup, and deploy-agent recovery should be delegated to the `claude-code-deploy-ops` lane with a redacted context pack. Codex/resident master supervises, validates evidence, and updates SSOT/evidence; routine shell/cloud operations should not stay in the Codex thread.

## CN Secret Store Note

`secret-store` is currently verified on the GCP xjp-backend VM as `https://ss.xiaojinpro.top`. A separate CN Secret Store remains a deployment target/design requirement, not a verified runtime, until deploy-center records a CN runtime target, database/KEK material, domain, health endpoint, and provenance.
