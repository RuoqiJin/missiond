# Secret Store CN runtime verification

Date: 2026-05-15

## Summary

`secret-store-cn` is running on Aliyun ECS and reachable through
`https://ss-cn.xiaojinpro.com`.

The runtime is verified by external health checks, but Deploy Center provenance
is still incomplete: `secret-store-cn` remains represented as a stale runtime
shell with `skip_deployment=true`, disabled stage configs, no deployed commit,
and no first-class OSS artifact record.

## Verification

```text
curl -i https://ss-cn.xiaojinpro.com/livez
HTTP/1.1 200 OK
...
OK

curl -i https://ss-cn.xiaojinpro.com/readyz
HTTP/1.1 200 OK
...
OK
```

## Runtime Facts

- Public endpoint: `https://ss-cn.xiaojinpro.com`
- Runtime target: Aliyun ECS `106.15.2.17`
- Work directory: `/opt/secret-store-cn`
- Reverse proxy: Nginx to `127.0.0.1:8091`
- Artifact lane used for first deployment: GCP `docker save` -> Aliyun OSS -> ECS `docker load`
- Deploy Center slug: `secret-store-cn`

## Remaining Governance Gap

Deploy Center must promote the ad-hoc OSS image transfer into a first-class
release provenance lane:

- source commit
- builder id
- OSS object key
- artifact sha256
- loaded image digest
- deploy-agent report
- smoke result
- rollback bundle

Until then, MissionD should classify this service as
`runtime-verified-with-provenance-gap`, not as `planned` or fully
Deploy-Center-governed.
