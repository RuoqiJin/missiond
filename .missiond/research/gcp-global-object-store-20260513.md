# GCP global object store evidence 2026-05-13

## Purpose

MissionD data-residency SSOT now treats the global XJP platform lane as `GCP VM + Google Cloud Storage`; the CN lane remains `Aliyun ECS + OSS`.

## Created bucket

- Bucket: `gs://xjp-global-object-store-project-20250408`
- GCP project: `project-20250408`
- Location: `ASIA`
- Storage class: `STANDARD`
- Uniform bucket-level access: enabled
- Soft delete retention: `604800s`

## Command evidence

The bucket was created through the local authenticated `gcloud` CLI. The active project before creation was `project-20250408`; the active account was `lamufbmstrf@gmail.com`.

## SSOT placement

- MissionD `infrastructure-universe`: `gcp-runtime` owns `google-cloud-storage` and `global-object-store` capability.
- MissionD `data-residency-universe`: `xjp-global-storage` points at this GCS bucket.
- Application partitions such as `pcea-global` bind to the XJP global platform storage ledger instead of inventing a per-app object store.

## Open items

- `global-eu` remains an operating-zone policy until a dedicated EU runtime and EU-pinned bucket are provisioned.
- Deploy Center should later become the runtime authority for bucket provenance and release-time storage binding.
