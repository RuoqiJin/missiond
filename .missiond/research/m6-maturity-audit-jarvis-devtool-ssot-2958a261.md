# M6 Maturity Audit: Jarvis Dev-Tool SSOT Batch

- BoardTask: `2958a261-ba03-43eb-a593-2a66fc003725`
- Audit date: 2026-05-05
- Scope: `/Users/jinchen/Projects/jarvis`, `/Users/jinchen/Projects/jarvis-forge`, `/Users/jinchen/Projects/jarvis-mechanic`, `/Users/jinchen/Projects/xjpcode`
- Read posture: project roots, `.missiond/**` blueprints/checkers/evidence, and explicit checker outputs only.
- Excluded: KB, historical conversations, provider durable logs, Board backlog, nightly/self-evolution.

## Verdict

All four projects are M6 for the SSOT maturity contract exercised by this audit. No visible remediation child BoardTasks are required.

The universe checker remains green for the four named entries, and each project-local checker passed in the current working tree. The review did not rely only on thin maturity labels: each project has concrete `entry/core/egress/surfaces/runtime-projection` structure plus checker coverage that pins the declared shape.

## Classification Matrix

| Project | Maturity | Entry | Core | Egress | Surface | Runtime | Checker | Missing M6 items |
|---|---:|---|---|---|---|---|---|---|
| `jarvis` | M6 | Covered by S5 routes and S7 tools | Covered by route/tool call chains | Covered by HTTP/SSE/EventBus/MCP/MissionD egress maps | Covered by HTTP, WS, IPC, MCP, EventBus, UI, workspace, external egress | Covered by S6 bootstrap DAG, workers, PTY state machine, lifetimes | `bash .missiond/check.sh`; `node scripts/check-jarvis-ssot.mjs` | None |
| `jarvis-forge` | M6 | Covered in backend/frontend L3 blueprints | Covered by `pillar-flow-map` steps | Covered per backend/frontend surface | Covered by L1 index plus backend/frontend L3 surface maps | Covered by runtime projections in both L3 blueprints | `bash .missiond/check.sh` including schema/isomorphism/complete gates | None |
| `jarvis-mechanic` | M6 | Covered in per-function blocks | Covered by ordered `s1..sN` function bodies | Covered per function | Covered by CLI/stdout/stderr/filesystem/subprocess declarations | Covered per function, including cleanup and subprocess behavior | `node scripts/check-mechanic-ssot.mjs` | None |
| `xjpcode` | M6 | Covered in `m6-overlay.pillar-map` | Covered per pillar function | Covered per pillar function | Covered by 26 checked surfaces and manifest module-map | Covered per pillar function, with config/storage/server/terminal projections | `node scripts/check-xjpcode-ssot-complete.mjs --json`; `node scripts/check-xjpcode-code-isomorphism.mjs --json` | None |

## Evidence

### `jarvis`

Files reviewed:
- `.missiond/intent.lisp`
- `.missiond/intent-surfaces.lisp`
- `.missiond/intent-runtime.lisp`
- `.missiond/intent-tools-map.lisp`
- `.missiond/intent-manifest.lisp`
- `.missiond/evidence/m6-convergence-report.md`
- `.missiond/evidence/m6-checker-first-ssot-shard.md`
- `.missiond/check.sh`
- `scripts/check-jarvis-ssot.mjs`

Live checker results:
- `bash .missiond/check.sh`: passed all default gates: shard existence, fingerprints, line drift, survey head, canonical root, crate boundary, tool/module count, diff check.
- `node scripts/check-jarvis-ssot.mjs`: passed path resolution for 137 source paths, tool count cross-shard check, body-status `current-code-mapping`, shard set coherence, full survey-head coherence.

M6 basis:
- `intent-manifest.lisp` declares `(granularity M6)` and `(maturity M6)`.
- S5 maps public surfaces with `:entry`, `:core`, and `:egress`.
- S6 provides runtime projection through bootstrap DAG, worker lifetimes, and PTY state.
- S7 maps 67 MCP tools with entry/IPC/MissionD egress/return-shape/side-effects.
- The legacy `intent.lisp` body remains explicitly `current-code-mapping`; M6 normalization is carried by the shard set and manifest, and the checker pins that contract.

Non-blocking deferred items recorded by the project: future full body promotion, virtual `tools/infrastructure` symbol expansion, DeltaValidator KB-resident rule authority, and opt-in heavy build gates. These are not missing M6 entry/core/egress/surface/runtime/checker items.

### `jarvis-forge`

Files reviewed:
- `.missiond/intent.lisp`
- `.missiond/backend/forge-backend-blueprint.lisp`
- `.missiond/frontend/forge-ui-blueprint.lisp`
- `.missiond/evidence/forge-ssot-convergence.lisp`
- `.missiond/evidence/m6-convergence-report.md`
- `.missiond/check.sh`
- `scripts/check-forge-ssot-complete.mjs`

Live checker result:
- `bash .missiond/check.sh`: passed Lisp schema, backend isomorphism, frontend isomorphism, `ssot-complete`, and diff check.

M6 basis:
- The root intent is a compact L1 blueprint with links to backend, frontend, and evidence L3 shards.
- Backend and frontend L3 blueprints carry `pillar-flow-map` entries with `:entry`, `:core`, `:egress`, `:surfaces`, and `:runtime-projection`.
- The checker validates schema and backend/frontend code isomorphism, then bundles them in `ssot-complete`.

No missing M6 items found.

### `jarvis-mechanic`

Files reviewed:
- `.missiond/intent.lisp`
- `.missiond/intent-manifest.lisp`
- `.missiond/intent-detail.lisp`
- `.missiond/evidence/m6-convergence-report.md`
- `scripts/check-mechanic-ssot.mjs`

Live checker result:
- `node scripts/check-mechanic-ssot.mjs`: passed shard existence, survey-head coherence, no-source-touched, no-full-fmt-run, governance enum coherence, governance mode field coverage, repair-cycle pipeline coverage, cargo-check oracle coverage, and declared downstream command coverage.

M6 basis:
- `intent.lisp` declares `(granularity M6)` and `maturity-target M6`.
- The body is an M6 pillar/function map. Each function uses the full `:entry / :core / :egress / :surfaces / :runtime-projection` quintet.
- `intent-manifest.lisp` declares `(maturity M6)`, a maturity matrix, evidence map, checker plan, and next gaps.
- The project is a single-binary CLI, so no backend/frontend split is required to satisfy the M6 surface model.

No missing M6 items found.

### `xjpcode`

Files reviewed:
- `.missiond/intent.lisp`
- `.missiond/intent-manifest.lisp`
- `.missiond/evidence/m6-convergence-report.md`
- `scripts/check-xjpcode-ssot-complete.mjs`
- `scripts/check-xjpcode-code-isomorphism.mjs`

Live checker results:
- `node scripts/check-xjpcode-ssot-complete.mjs --json`: passed with `hard_failures=0`, including M6 overlay presence, pillar coverage, governance gates, version pin, dirty-baseline policy, and dirty-baseline match.
- `node scripts/check-xjpcode-code-isomorphism.mjs --json`: passed with `26/26` overlay surfaces present, `31/31` module-map paths present, dirty-baseline paths present, and version pin `0.5.3`.

M6 basis:
- `intent-manifest.lisp` declares `(maturity M6)`.
- `intent.lisp` has an `(m6-overlay ...)` declaring `(maturity M6)`, granularity `pillar -> function -> entry/core/egress -> surface`, governance gates, version pin, dirty-baseline policy, and next gaps.
- The overlay includes concrete pillar functions for CLI, TUI runtime, rendering, router streaming, MCP tool loop, command dispatch, context refs, compaction, transcript persistence, tool-result storage, app state, config, briefing loader, HTTP server mode, content parser, and scroll acceleration.
- The code-isomorphism checker confirms the declared surfaces exist on disk even with the operator-owned dirty baseline.

No missing M6 items found. The dirty-baseline cleanup and future tighter clean-tree code alignment are next-stage tasks recorded in the manifest, not blockers for current M6 classification.

## Checker Summary

Commands run in this audit:

```sh
cd /Users/jinchen/Projects/jarvis && bash .missiond/check.sh
cd /Users/jinchen/Projects/jarvis && node scripts/check-jarvis-ssot.mjs
cd /Users/jinchen/Projects/jarvis-forge && bash .missiond/check.sh
cd /Users/jinchen/Projects/jarvis-mechanic && node scripts/check-mechanic-ssot.mjs
cd /Users/jinchen/Projects/xjpcode && node scripts/check-xjpcode-ssot-complete.mjs --json
cd /Users/jinchen/Projects/xjpcode && node scripts/check-xjpcode-code-isomorphism.mjs --json
cd /Users/jinchen/Projects/missiond && node scripts/check-project-ssot-universe.mjs --json
```

All commands exited `0`. The universe checker returned `ok=true`, `diagnostics=[]`, and green entries for `jarvis`, `jarvis-forge`, `jarvis-mechanic`, and `xjpcode`.

## Dirty Baseline Notes

- `jarvis`: non-source operator notes/cache/docs remain dirty; SSOT checkers passed and no project files were edited.
- `jarvis-forge`: clean working tree before and after audit reads.
- `jarvis-mechanic`: only untracked `.claude/`; checker pins that source hygiene posture.
- `xjpcode`: declared operator-owned tracked/untracked WIP remains present; SSOT and isomorphism checkers both passed against it.

## Decision

`close_or_backfill`: write this evidence artifact and close BoardTask `2958a261-ba03-43eb-a593-2a66fc003725`. No child tasks are created because no M6-blocking missing entry/core/egress/surface/runtime/checker item was found.
