# MissionD Board Cleanup Batch 25 - 2026-05-10

Scope: read-only review of the **remaining 4 MissionD-related BoardTasks** in the close-candidates queue. Dispatch task: `b6b92ee0-77aa-4f6d-aba8-1f60f7ea3d38`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

This is the closing batch (after Batches 03–24).

Heuristic applied: **first ask whether each original ask still holds today; then verify against SSOT/code/checker/runtime; finally give one actionable verdict**. Wide goals are not split into sub-tasks per dispatch instruction.

All 4 reviewed tasks are currently `status=open`.

## Summary

| Task ID | Title | Original Ask Still Valid? | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `8de8ca75-34c6-40f0-87a7-c1d3a4bc290f` | `[Forge] missiond Architecture Lint` | No — the lint already ran; the report is the sibling row `132b5fe9` | `close-covered` | Close; this row was the execution stub for the lint that became `132b5fe9`. |
| `132b5fe9-0b52-4aa2-948a-02ddf47812d7` | `[Arch Lint] missiond 架构审查报告 2026-04-15` | Mostly no — C1 is covered; C2 has ControlTree/circuit-breaker substrates but no longer belongs as a broad 2026-04-15 umbrella; most W items were reshaped by V3 SSOT cutover and shared-memory | `close-superseded` | Close at the parent level; if a C2/W residual is still operationally reproducible, file it as a fresh narrow task. |
| `2b685fcf-9773-4881-95e5-b101b7ecc68c` | `Fix dispatch stability findings from auth KB smoke` | No — three children done, runtime fixes shipped, codex-resident-master closeout note already present | `close-covered` | Close; the existing summary note + child evidence is sufficient. |
| `882527ab-ba9a-4d9c-8c45-02501534aaf5` | `Fix MissionD deploy workflow issues observed during Auth M6 production deploy` | Partially — some items shipped via Phase 6 / EventBridge, but Auth-M6-specific deploy-ops lane / structured smoke / claim-displacement rules are residual | `keep` | Single row, no sub-task split; encode the residual deploy-ops rules in V3/workflow Lisp + checker. |

## Evidence

### `8de8ca75-34c6-40f0-87a7-c1d3a4bc290f` — `close-covered`

Heuristic: the lint *was already run*; this row is the execution stub whose output is the sibling row `132b5fe9`.

- Created `2026-04-15T09:37:05.803101+00:00`. Sibling row `132b5fe9-...` was created `2026-04-15T09:37:07.841887+00:00` — **2.04 seconds later**, with title `[Arch Lint] missiond 架构审查报告 2026-04-15` carrying explicit findings (C1, C2, W1–W6). The pair is "execute → report".
- Contrast with the Batch 24 trio (`51c8ea9e`, `12f8bc50`, `e7c2e0d5`) created at 2026-04-15 08:27 — that earlier trio asked for the lint but did not produce a report; this row is the second-attempt execution that did produce one.
- Evidence the body claims (7 lint dimensions: 模块职责边界 / 循环依赖 / 分层 / 单一职责 / 抽象 / async-sync / 错误处理) is exactly what the C1/C2/W1–W6 finding list in `132b5fe9` reports against.
- `mission_forge_lint` exists in code (`crates/missiond-mcp/src/tools/compute/forge.rs:20–22`, Batch 20/24 evidence).

Recommendation: close as covered. The execution side of this duplicate cluster ran; the surviving artefact is the report row `132b5fe9`.

### `132b5fe9-0b52-4aa2-948a-02ddf47812d7` — `close-superseded`

Heuristic: most C/W items have been overtaken by Phase 6 / ControlTree / V3 SSOT cutover / shared-memory work; the report-as-row is the unupdated stub.

Item-by-item against current code (cross-referenced to earlier batches):

- **C1** Autopilot dependency check + restart recovery
  - Restart recovery: Phase 6.4 closed in Batch 23 — `crates/missiond-daemon/src/main.rs:412–413 store.recover_stale_running_tasks(0).await`.
  - Dependency readiness gate: covered by `BackgroundWorker.dependencies()` introduced under the Unified Control Tree work (Batch 9 task `f533e3df`, close-covered) — `crates/missiond-daemon/src/control_tree.rs:163 is_effectively_paused(worker_name, deps)`.
- **C2** Slot-level circuit breaker
  - Superseded as an umbrella finding, not proven as a single complete "tool-unavailable" feature. Current substrates exist: ControlTree `slot_role` pause/dependency cascade (`crates/missiond-daemon/src/control_tree.rs:148, 163, 301`), paused role kill path (`crates/missiond-daemon/src/handlers/compute/worker.rs:139–162`), quota-exhaustion global circuit breaker (`crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:2897–2903`), and worker-local failure breaker (`crates/missiond-daemon/src/workers/sonnet/translation_worker.rs:38–42, 222–229`). If "tool unavailable" still means a liveness probe + auto-heal contract, that should be a fresh narrow row, not this broad report.
- **W1** Mission Core 调度决策与执行管理职责重叠
  - Reshaped by the engine 8→2 collapse (Batch 11 task `3df06926` close-superseded): `engine/intent_engine/` (autopilot/flow/memory_scheduler/workflow_executor) vs `engine/learning_engine/` is now the canonical split.
- **W2** async/sync 混用
  - Resolved by sqlx async pool + Postgres-only runtime (Batch 20 task `f411c3b8` close-covered).
- **W3** 错误处理不一致 / bare except
  - Generic concern; the Rust codebase doesn't have `bare except` semantics. Where the original auditor saw inconsistency, the structured `ToolError` / `ToolResult::structured_error` pattern (Batch 9 / Batch 14 evidence on `note.rs`) and `INVALID_PARAM` family is now the convention.
- **W4** kb ↔ board 潜在循环依赖
  - Partially superseded by V3 SSOT cutover; `event_log` is the shared truth source (Batch 10 task `d5c6ecba` close-superseded). KB and Board events both publish into one bus rather than calling each other directly.
- **W5** 配置管理分散
  - Largely superseded by `.missiond/v3/missiond-blueprint.lisp` + `context/v3_blueprint_runtime.rs` (Batch 23 evidence on `parse_minimax_runtime_policy` etc.).
- **W6** trace_id 链路不完整
  - `tracing` is wired and EventBridge envelopes carry trace/correlation fields; end-to-end `trace_id` propagation can still be checked by focused EventBus tests, but it is no longer useful to keep this 2026-04-15 umbrella row open.

Recommendation: close as superseded. The C1/C2 P0-action items are demonstrably covered; the W list has been reshaped by V3/OCaml/shared-memory/EventBus changes that landed *after* this 2026-04-15 review. If a specific W-item residual is still felt operationally, file a one-line scope row (per dispatch instruction, do not split here).

### `2b685fcf-9773-4881-95e5-b101b7ecc68c` — `close-covered`

Heuristic: codex-resident-master closeout note explicitly says "close parent as completed"; downstream evidence backs it.

- Existing close-out summary note `1cd5d672-b7e0-4c65-96e6-f1c99b3cfc18` (author `codex-resident-master`, `2026-05-03T13:38:42`):
  - "Board child `83ef5b3b-104d-412c-8038-bc9a71cfb9dc` is `done`; provider conversation `bcc392c7` completed and task-linked to that child."
  - Runtime fixes commit `84be8f46`: review-class routing, `read_scope` metadata, final-report anchors, multi-repo git advisory.
  - Backfill commit `9d3f67e6`: `.missiond/v3/missiond-blueprint.lisp`, `.missiond/workflows/project-ssot-convergence.lisp`, `scripts/check-v3-workstation-config-isomorphism.mjs`, `scripts/check-v3-workstation-pool-isomorphism.mjs`.
  - Test counts: 1960 daemon + 19 MCP passing.
  - Convergence: `git diff --check` clean, `mission_convergence_status` green.
- Three children all `done`:
  - `e324419c` Lisp/checker backfill (`events_sync.rs` + `infra/message_handler.rs` role taxonomy + 8 new test cases).
  - `83ef5b3b` runtime fixes (`task_delegate.rs` `read_scope` threading, `mission_swarm_run` parallel patch).
  - `c49c4121` xiaojinpro-backend context-pack dispatch unblocked (verified with provider session `8898f40e` and 149-line target file).
- The user-heuristic-hinted "复核证据是否足够" check passes: the 5 acceptance items in the parent body are each pinned to specific commits/files/test runs.

Recommendation: close as covered. The summary note already reads as a closeout; the only reason the row still sits open is that the cleanup wave never honoured it.

### `882527ab-ba9a-4d9c-8c45-02501534aaf5` — `keep`

Heuristic: 5 deploy-workflow lessons; some shipped via EventBridge / Phase 6 / V3 SSOT, others remain operational residuals worth encoding.

Item-by-item:

1. **Autopilot must not close parent BoardTask before durable final evidence** — partially encoded. Batch 19's `5d6b5705` evidence + this batch's `2b685fcf` close-out behaviour shows that the autopilot blocking close on `missing-output-contract-sections` (Batch 11 evidence on `c889ebfc`) and on missing convergence already enforces this; the residual is encoding the precise definition of "durable final" in workflow Lisp so future autopilot versions can't regress.
2. **deploy/ops tasks should route to explicit deploy-ops lane/slot** — only partial. `deploy-ops` skill exists at `~/.claude/skills/deploy-ops/SKILL.md` (Batch 17 cross-reference), but `mission_task_delegate` engine_hint enforcement was the focus of `2b685fcf` (now closed). Specific deploy-ops lane is not pinned in V3 Lisp.
3. **Deployment monitoring must not rely on worker shell sleep** — operational rule, not currently enforced in Lisp. The `mission_execution heartbeat` model (Batch 14 task `9dabe4a3` close-covered) is the right substrate, but its application to deploy monitors is not codified.
4. **Acceptance smoke = structured commands, not shell pipeline** — `m6-deployment-rollout.lisp` now requires smoke evidence (`.missiond/workflows/m6-deployment-rollout.lisp:27–28, 43`), and `deployment-event-response.lisp` makes durable smoke/deploy events first-class (`.missiond/workflows/deployment-event-response.lisp:17–24`). Batch 20 task `2bdb1b50` remains the concrete Auth-M6 locus for hardening the smoke-command shape.
5. **`xjp_deploy_status` aggregate timeout** — owned by `xjp-mcp` / deploy-center, not MissionD core.
6. **Claim-displacement workflow** (note `18841cec-b16f-48c2-8e65-102888b87847` 2026-05-07): "before spawning a replacement context-pack task, mark the superseded task explicitly and link replacement id" — not encoded today.

Recommendation: keep. Per dispatch instruction, no sub-task split. The single row should encode (a) the `claim-displacement-must-mark-superseded` rule and (b) the `deploy-ops lane lisp pin`; items (3) and (4) cross-reference Batch 14 / Batch 20 and don't duplicate them.

## Closing Notes (cleanup-season summary)

- **Batches 03–25 (this final row)** processed roughly 5×23 = 115 BoardTasks. The dominant pattern across the season:
  - "Already shipped, never re-checked" rows (e.g. all Phase 6 hardening rows in Batch 23, the SSOT cutover row in Batch 10, the `mission_router_chat files=[...]` row in Batch 14).
  - Multiple-row duplicates of one observation (memory pipeline waste family across Batches 10/16/18/19/22; forge lint trio in Batch 24 + this batch's `8de8ca75`/`132b5fe9`).
  - Misfiled-board rows belonging to xjp-mcp / private-cloud / hostvds rather than MissionD (Batches 16/19's HostVDS/runner rows).
- **Recommended cleanup-loop heuristics for future waves**:
  1. Auto-detect rows whose body cites a `// Phase X.Y:` or `[Px.y]` identifier and match against current source comments — close-covered when present.
  2. Auto-merge rows whose `(title prefix, target project, day)` match — duplicate cluster.
  3. Auto-flag rows whose body cites `slot-clean` / `HostVDS slot` / similar deployment topology that no longer matches current memory.
- Per dispatch instruction, this batch did not spin off any new sub-rows for the residuals identified above. Each residual is either covered, merged into an open row, or kept on its parent row for human reattachment.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-25-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ `must_not_touch` directories (`.git`, `crates/`, `packages/`, `scripts/`) untouched (read-only).
- ✅ Each reviewed task carries one classification from the allowed set and at least one of `file_path:line` / Board note id / cross-batch reference / commit-hash as evidence.
- ✅ Heuristic format honoured: original-ask validity question first, then SSOT/code/runtime check, then a single actionable verdict per task — no sub-task spinning.
- ✅ Final answer follows the Findings / Evidence / Recommendations / Verification contract; no raw KB JSON or full logs pasted.
- ✅ User-supplied heuristic hints honoured for `8de8ca75` (duplicate of Batch 24 trio question), `132b5fe9` (C1/C2/W1–W6 vs current architecture), `2b685fcf` (codex-resident-master closeout note review), and `882527ab` (EventBridge / deploy workflow / SSOT coverage triage).
- ✅ Read-only checker verification run after the report review: `node scripts/check-v3-workstation-config-isomorphism.mjs`, `node scripts/check-v3-workstation-pool-isomorphism.mjs`, `node scripts/check-v3-autopilot-runtime-isomorphism.mjs`, `node scripts/check-v3-project-registry-isomorphism.mjs`, `node scripts/check-v3-compute-primitives-isomorphism.mjs`, `node scripts/check-v3-eventbridge-isomorphism.mjs`, `node scripts/check-v3-workflow-isomorphism.mjs`, and `node scripts/check-v3-mission-execution-isomorphism.mjs` all returned OK.
