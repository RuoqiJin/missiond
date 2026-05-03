# MissionD Deploy Speed Report - 2026-05-03

## Current Timing Snapshot

Latest observed debug blue-green deploy timing:

| Stage | Time |
| --- | ---: |
| cargo-build | 30s |
| launchd | 3s |
| socket-wait | 2s |
| post-smoke | 5s |

The dominant cost is local Rust compilation. Launchd restart, socket readiness,
and MCP smoke are already small compared with `cargo build`.

## Implemented

- Blue-green release layout under `~/.xjp-mission/releases/<release-id>`.
- `active` symlink cutover with stable `~/.xjp-mission/missiond` and
  `~/.xjp-mission/mission-mcp` entrypoints.
- Release manifest and release-id consistency checks.
- Rollback path when post-cutover smoke fails.
- Cleanup reporting and guarded cleanup apply.
- `scripts/deploy-daemon.sh --debug` timing summary.
- `scripts/deploy-daemon.sh --fast` dev path.
- `MISSIOND_USE_SCCACHE=1` switch for local compiler cache experiments.

## Not Yet Proven

- `sccache` hit rate has not been measured across repeated deploys.
- No `kellnr` integration is active.

## Assessment

The next optimization should verify `sccache` first. `kellnr` helps with crate
registry/download caching, but the current bottleneck is local incremental
compilation (`cargo-build=30s`), not dependency download. If `sccache` shows a
low hit rate, the next report should inspect `RUSTC_WRAPPER`, cache directory,
and whether debug/release flags or env changes are invalidating the cache.

## Next Measurement

Run two consecutive debug deploys:

```bash
MISSIOND_USE_SCCACHE=1 scripts/deploy-daemon.sh --debug
sccache --show-stats
MISSIOND_USE_SCCACHE=1 scripts/deploy-daemon.sh --debug
sccache --show-stats
```

Record:

- compile requests
- cache hits/misses
- non-cacheable reasons
- total cargo-build time per run

Only after this should MissionD decide whether to keep `sccache`, tune it, or
look at registry/cache infrastructure such as `kellnr`.
