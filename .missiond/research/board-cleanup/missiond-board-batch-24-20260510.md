# MissionD Board Cleanup Batch 24 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. Dispatch task: `fcb76f6d-a2b0-4257-bad0-88d46b5f31b5`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

Heuristic applied: **first ask whether each original ask still holds today; then verify against SSOT/code/checker/runtime; finally give one actionable verdict**. Wide goals are not split into sub-tasks per dispatch instruction.

This batch contains a **near-duplicate cluster**: three rows (`51c8ea9e`, `12f8bc50`, `e7c2e0d5`) all request a Forge architecture lint on the missiond project, all created within seconds on 2026-04-15. They are treated as "canonical + 2 duplicates".

All 5 reviewed tasks are currently `status=open`.

## Summary

| Task ID | Title | Original Ask Still Valid? | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `ed52268b-752e-4b43-86f2-82221d33c1c1` | `[P6.7] 动态 Slot 孤儿进程回收 — Daemon 重启对账` | No — Phase 6.7 startup termination shipped | `close-covered` | Close; `main.rs:418` carries the `Phase 6.7: Terminate ALL active dynamic slots on daemon restart.` block. |
| `61a1994d-5a93-4ce3-9e05-a4b4aa8851f0` | `Claude Code API Relay — Router-managed 住宅 IP 保护` | No — relay shipped end-to-end via HostVDS Caddy + xjp-router AnthropicDirect + IPRoyal residential | `close-covered` | Close; `hostvds/SKILL.md` documents `relay.xiaojinpro.top:9445 → 127.0.0.1:9446 (API Relay)` and `IPRoyal SOCKS5 38.15.20.134`. |
| `51c8ea9e-f03c-4207-8aa2-b86f0c8d569a` | `missiond 架构 Lint 分析` | Yes — `mission_forge_lint` exists, but the missiond run + report has not happened | `keep` | Treat this row (earliest of the three duplicates) as the canonical execution row. |
| `12f8bc50-a494-4dbe-a8c5-1540b1c1af16` | `[Forge] missiond Architecture Lint` | Duplicate of `51c8ea9e` (same day, +7 seconds) | `merge-into-existing-candidate` | Merge into `51c8ea9e`; do not run twice. |
| `e7c2e0d5-8f89-4191-ba63-57dcc8b2f0a3` | `[Forge] missiond Architecture Lint` | Duplicate of `51c8ea9e` (same day, +7.6 seconds) | `merge-into-existing-candidate` | Merge into `51c8ea9e`; do not run twice. |

## Evidence

### `ed52268b-752e-4b43-86f2-82221d33c1c1` — `close-covered`

Heuristic: design doc's #6.7 has shipped exactly where Phase 6.4 / 6.5 / 6.8 also live.

- `crates/missiond-daemon/src/main.rs:418` carries the explicit comment `// Phase 6.7: Terminate ALL active dynamic slots on daemon restart.` — i.e. the startup-time orphan-slot reconciler.
- `:437–438` documents the next phase: `Phase 6.8: Clear BoardTask 'assignee' pointers that reference dynamic slots which are no longer active. After Phase 6.7 terminates active …` — confirming that 6.7 runs *before* 6.8 in the same startup sequence.
- The orphan-pid scan at `:1065–1066` (`let orphan_pids: Vec<&&str> = pids.iter().filter(|p| **p != my_pid).collect(); if !orphan_pids.is_empty() { ... }`) is the cross-process check that ensures stale PTY processes are not silently kept across daemon restart.
- Companion design doc reference: `docs/designs/phase6-hardening.md` (verified in Batch 23 to exist).

Recommendation: close as covered.

### `61a1994d-5a93-4ce3-9e05-a4b4aa8851f0` — `close-covered`

Heuristic: the 5-phase plan has shipped end-to-end; the row is the unupdated stub.

- xjp-router connector is on:
  - `~/.claude/skills/services/router/SKILL.md:53` lists `AnthropicDirect | Anthropic API relay | ON | anthropic-relay (passthrough) | anthropic-shared`.
- HostVDS deployment:
  - `~/.claude/skills/hostvds/SKILL.md:84` lists `9446 | 127.0.0.1 | xjp-router | API Relay (AnthropicDirect)` — the local Anthropic relay port.
  - `:129` documents the full path: `relay.xiaojinpro.top:9445 → TLS(Let's Encrypt DNS-01 Cloudflare) → 127.0.0.1:9446 (API Relay)`.
  - `:135` notes `Let's Encrypt：relay.xiaojinpro.top、st.xiaojinpro.top（DNS-01 Cloudflare，自动续期）` — Phase 1 (xcaddy + Cloudflare DNS module) shipped.
  - `:177` confirms residential egress: `出站流量全部走 IPRoyal 住宅代理（SOCKS5 38.15.20.134:443），出口 IP: 38.15.20.134.`
  - `:215` records the client-side env: `ANTHROPIC_BASE_URL=https://relay.xiaojinpro.top:9445/relay`.
- Cross-skill confirmation:
  - `~/.claude/skills/missiond/SKILL.md:1379` references `model: sonnet  # 经 xjp-router cpapi-claude-sonnet` — actual usage.
  - `~/.claude/skills/openclaw/SKILL.md:57, 418` lists `xjp-router/cpapi-claude-sonnet` as recommended/default.

Recommendation: close as covered. The Gemini review concerns the row body lists (SSE billing, DB latency) belong to xjp-router's own Board, not this row.

### `51c8ea9e-f03c-4207-8aa2-b86f0c8d569a` — `keep`

Heuristic: the lint *tool* is in code; the lint *execution* + report has not happened. This row is the canonical execution row.

- `crates/missiond-mcp/src/tools/compute/forge.rs:20–22` defines `mission_forge_lint` (Batch 20 evidence covered the build counterpart `mission_forge_build`).
- The task asks for an analysis report against `/Users/jinchen/Projects/missiond` covering modules / cycles / layering / organization — that is one `mission_forge_lint` invocation on the missiond project, plus a written report.
- Created `2026-04-15T08:27:21.037260+00:00`, **3.6 seconds before** the next duplicate (`12f8bc50` at `:28.604462`) and **7.8 seconds before** `e7c2e0d5` at `:28.858965`. Earliest creation → canonical.

Recommendation: keep. Per dispatch instruction, do not split into sub-tasks; one `mission_forge_lint` run + one written report closes it.

### `12f8bc50-a494-4dbe-a8c5-1540b1c1af16` — `merge-into-existing-candidate`

Heuristic: textual duplicate of `51c8ea9e` with the same execution plan and tool target.

- Title `[Forge] missiond Architecture Lint`; body lists the same 5 steps (file structure scan, dependency analysis, cycle detection, compliance evaluation, report).
- Same project root: `/Users/jinchen/Projects/missiond`.
- Same tool: `mission_forge_lint (architecture mode)`.
- Created `2026-04-15T08:27:28.604462+00:00`, 7.4 seconds after `51c8ea9e`.

Recommendation: merge into `51c8ea9e`. Running the lint twice is pure waste.

### `e7c2e0d5-8f89-4191-ba63-57dcc8b2f0a3` — `merge-into-existing-candidate`

Heuristic: another textual duplicate of the same intent, created seconds later.

- Title `[Forge] missiond Architecture Lint`; body lists the same 5 steps as `12f8bc50` with one fewer line of preface.
- Same project root and same tool target.
- Created `2026-04-15T08:27:28.858965+00:00`, 0.25 seconds after `12f8bc50` and 7.8 seconds after `51c8ea9e`.

Recommendation: merge into `51c8ea9e` (or `12f8bc50` if the later cleanup wave reverses canonicalisation; date-of-creation is the simpler tie-breaker).

## Notes

- Two of the five rows (`ed52268b`, `61a1994d`) were never re-checked after their implementation shipped. Both have unambiguous cross-skill / `// Phase 6.x:` evidence; the cleanup wave should auto-detect such rows by matching `// Phase N.M:` comments to open Board rows naming the same identifier. (Batch 23 already noted this for P6.2 / P6.4 / P6.5.)
- The `51c8ea9e` / `12f8bc50` / `e7c2e0d5` triple is the cleanest example of duplicate-row creation in a single session; the cleanup wave should auto-merge by `(title prefix, target project, day)` regardless of natural-language differences. Per dispatch instruction, no new sub-rows are created here.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-24-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ `must_not_touch` directories (`.git`, `crates/`, `packages/`, `scripts/`) untouched (read-only).
- ✅ Each reviewed task carries one classification from the allowed set and at least one of `file_path:line` / skill-file reference / creation-timestamp / cross-batch fact as evidence.
- ✅ Heuristic format honoured: original-ask validity question first, then SSOT/code/runtime check, then a single actionable verdict per task — no sub-task spinning.
- ✅ Final answer follows the Findings / Evidence / Recommendations / Verification contract; no raw KB JSON or full logs pasted.
