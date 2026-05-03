# MissionD Deploy Speed Report - 2026-05-03

## Current Timing Snapshot

Latest observed debug blue-green deploy timing with `MISSIOND_USE_SCCACHE=1`:

| Stage | Time |
| --- | ---: |
| cargo-build | 1s |
| release-copy | 0s |
| codesign | 1s |
| pre-switch-mcp-smoke | 0s |
| launchd | 3s |
| socket-wait | 2s |
| post-switch-mcp-smoke | 8s |
| cleanup | 0s |

The full debug blue-green deploy promoted release
`20260503T133525Z-9d3f67e6aded-debug`. The previous observed slow path was
`cargo-build=30s`; the newest full deploy was fast because Cargo had already
warmed its incremental state in two preceding build-only runs.

## SCCache Measurement

Two consecutive build-only runs were measured after `sccache --zero-stats`:

| Run | cargo-build | sccache compile requests | cache hits | cache misses | non-cacheable |
| --- | ---: | ---: | ---: | ---: | ---: |
| build-only #1 | 37s | 6 | 0 | 0 | 6 |
| build-only #2 | 1s | 6 | 0 | 0 | 6 |

`sccache --show-stats` reported non-cacheable reasons:

- `crate-type`: 2
- `missing input`: 2
- `-`: 1
- `incremental`: 1

Conclusion: this local debug deploy path is not currently benefiting from
sccache. The second run is fast because Cargo incremental compilation is warm,
not because sccache is producing hits. For this repository's current local
iteration loop, preserving incremental build state matters more than adding a
registry/cache layer.

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

- `sccache` may still help clean or CI-like builds if incremental is disabled,
  but it did not help the measured local debug deploy path.
- No `kellnr` integration is active.

## Assessment

Keep `MISSIOND_USE_SCCACHE=1` as an opt-in experiment, but do not treat it as
the default deploy-speed fix yet. `kellnr` helps with crate registry/download
caching, but the current measured bottleneck is local compilation and smoke
latency, not dependency download.

The next speed gains should come from:

- Avoiding unnecessary clean builds and preserving Cargo incremental state.
- Measuring MCP smoke latency separately; the latest post-switch MCP smoke was
  8s, which is now larger than a warm cargo build.
- Considering a dev-only restart path that reuses an already built binary when
  `git rev-parse HEAD` and binary mtime prove no Rust change occurred.

## Next Measurement

If sccache is revisited, measure clean and incremental separately:

```bash
sccache --zero-stats
MISSIOND_USE_SCCACHE=1 scripts/deploy-daemon.sh --debug --build-only
sccache --show-stats
MISSIOND_USE_SCCACHE=1 CARGO_INCREMENTAL=0 scripts/deploy-daemon.sh --debug --build-only
sccache --show-stats
```

Record:

- compile requests
- cache hits/misses
- non-cacheable reasons
- total cargo-build time per run

Only after this should MissionD decide whether to keep `sccache`, tune it, or
look at registry/cache infrastructure such as `kellnr`.
