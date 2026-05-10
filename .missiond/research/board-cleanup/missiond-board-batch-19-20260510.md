# MissionD Board Cleanup Batch 19 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. Dispatch task: `96a70f50-33fa-4925-a028-c4ec9189c9c7`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

Heuristic applied: **first ask whether each original ask still holds today; then verify against SSOT/code/checker/runtime; finally give one actionable verdict**. Wide goals are not split into sub-tasks per dispatch instruction.

All 5 reviewed tasks are currently `status=open`.

## Summary

| Task ID | Title | Original Ask Still Valid? | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `57304f76-2e1c-408c-a723-7d73bb7496d4` | `HostVDS 需要 deploy-agent 或 MCP 工具减少 SSH 操作开销` | Yes — but the implementation lives outside MissionD | `rewrite-candidate` | Move to `xjp-mcp` / `hostvds` / `xjp-deploy-agent`; MissionD has no surface to land it. |
| `e988ada0-8949-41a6-907b-d4012f3e3b55` | `Board dependsOn 字段缺乏 UUID 校验` | Yes — confirmed live; no validator on the create/update path | `keep` | Single-file patch in `handlers/knowledge/board/`; reject `dependsOn` IDs not present in `board_tasks`. |
| `c8c0345a-68db-432f-a122-98410fe46908` | `Realtime-extract batch 消息重叠优化：dispatch 去重 + 水位线提前推进` | Partially — same memory-pipeline waste family already kept | `merge-into-existing-candidate` | Merge into `22661290` / `905e5a26` family; the "in-flight watermark" residual is one of the same leaves. |
| `d1ffe953-c6f5-4aba-8e57-9c6496457c38` | `Realtime 调度器空批次浪费：30:1 空转比` | Partially — empty-pending early-exit already exists for deep-analysis; realtime side benefits from `conversation_type='user'` gate | `merge-into-existing-candidate` | Merge into `2de8cbb2` (exponential backoff) and `dd65b5eb` (cooldown + memory_pending error); the "30:1" framing duplicates existing tracked work. |
| `f5e50d2d-7c3f-48bc-8d71-1f6bd48c3e72` | `Daemon 优雅自部署：Slot 触发 binary 替换不中断 MCP` | Yes — `mission_daemon_update` is blue-green-only; in-process exec replacement is missing | `keep` | Single-thread residual: add a `mission_daemon_upgrade` path with SIGUSR1 + `exec` fd inheritance, or document blue-green as the official answer. Do not split. |

## Evidence

### `57304f76-2e1c-408c-a723-7d73bb7496d4` — `rewrite-candidate`

Heuristic: ask is real and unfixed; surface lives outside MissionD.

- HostVDS skill exists at `~/.claude/skills/hostvds/SKILL.md`, but does not name an `xjp_agent_exec` endpoint for HostVDS.
- `xjp_agent_exec` is referenced in adjacent skills (aliyun, pcea, services/router, deploy-ops, services/rustdesk) — confirming the tool exists but only for ECS-style hosts.
- Project memory pins MissionD off HostVDS: `MissionD 部署在本机 Mac (与 Claude Code 同机 IPC 通信)，不在 HostVDS。HostVDS(45.156.24.163) 运行 OpenClaw 等独立服务。`
- The 30+ raw `sshpass` commands the task names are an operator pain in the `hostvds` / `xjp-mcp` workflow, not a missing surface in `crates/missiond-*`.
- This is the same shape as Batch 16 task `233c10f9` (Claude Max OAuth relay on HostVDS) and Batch 16 task `8eff0df1` (GA runner stale-busy) — all infrastructure rows misfiled to MissionD's Board.

Recommendation: rewrite. Move the row to the right Board (xjp-mcp / hostvds / xjp-deploy-agent). MissionD will not implement or close it as long as it sits here.

### `e988ada0-8949-41a6-907b-d4012f3e3b55` — `keep`

Heuristic: bug is concrete; validator missing.

- `crates/missiond-core/src/types/board.rs:254` declares `pub depends_on: Vec<TaskId>` on the canonical `BoardTask`; `:386–387` and `:447–448` carry `pub depends_on: Option<Vec<String>>` on the create/update inputs (camelCase `dependsOn`).
- `rg "dependsOn|depends_on"` across `crates/missiond-daemon/src/handlers/knowledge/board/` returns only the documentation string in `decompose.rs:77, 85` (`"用 dependsOn 串联，确保执行顺序"`), no validator.
- `update.rs` and `create.rs` accept `dependsOn` as an opaque `Vec<String>` and persist it without checking each entry against `board_tasks` row IDs.
- The 2026-03-14 reproduction (`session c705f2b9`) is consistent: setting `dependsOn: ["P1"]` is silently accepted; Autopilot then fails to resolve "P1" and the dependent task is permanently `blocked`.

Recommendation: keep. Single-function patch: in `board_create_args` / `board_update_args` ingestion, after parsing, for each `dependsOn` entry call `state.store.get_board_task(&id)` (already used elsewhere in `note.rs:67`) and reject with an `INVALID_PARAM` structured error if missing. No sub-task needed.

### `c8c0345a-68db-432f-a122-98410fe46908` — `merge-into-existing-candidate`

Heuristic: same memory-pipeline waste family already kept open.

- The 40–60% cross-batch overlap the task names is the watermark-confirmation gap: `realtime_forwarded_at` only advances after the worker confirms (Batch 16 evidence — `update_realtime_forwarded_at` is the writer at the end of pending fetch path).
- No in-flight set / dispatch dedup is implemented:
  - `rg "in_flight|inflight|already_dispatched|pending.*dispatch.*check"` in `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` returns empty.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:116 check_realtime_extraction` uses `try_claim_extraction_probe` for *concurrency* serialization but not for *message-id-level* dedup.
- Open peer rows that already track this family:
  - `22661290-d93e-4734-9a1f-8a95fcb9c946` (Batch 16 `keep`, narrowed to memory-slot self-exclusion).
  - `905e5a26-73e1-44dd-8098-4e41340276f8` (Batch 16 `merge`, deep-analysis 75% empty).
  - `2de8cbb2-...` (Batch 10 `keep`, exponential backoff).
  - `dd65b5eb-...` (Batch 10 `keep`, cooldown + memory_pending error).
  - `a861cebf-5268-4484-a41f-bfb3c9fcf72e` (Batch 18 `merge`, 85–93% empty rate).

Recommendation: merge into the family. The "optimistic watermark / in-flight set" residual is one specific leaf and belongs under `22661290` rather than as its own row.

### `d1ffe953-c6f5-4aba-8e57-9c6496457c38` — `merge-into-existing-candidate`

Heuristic: empty-pending early-exit already exists for the slow lane; the fast lane benefits structurally from `conversation_type='user'` gate; the residual is backoff/cooldown.

- Empty-pending early-exit confirmed for deep-analysis:
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:498–510` `if pending_convs.is_empty() { release_extraction_probe(...); return; }`.
- Realtime gate is structural: `get_pending_realtime_messages_with_limit` already filters `WHERE c.conversation_type = 'user'` (Batch 16/17/18 evidence at `crates/missiond-core/src/db/pg/conversation.rs:1542`).
- The "30:1" empty rate the task observes is therefore not a missing pre-check — it is a missing **backoff** when the pre-check returns empty repeatedly:
  - `2de8cbb2-...` (Batch 10) explicitly tracks the `3s → 6s → 12s → 30s → 60s` exponential backoff.
  - `dd65b5eb-...` (Batch 10) tracks `last_completion_at` cooldown + `mission_memory_pending` error response.
- Combined with the in-flight residual under `22661290` / `c8c0345a`, this row's framing is fully covered across kept rows.

Recommendation: merge into `2de8cbb2` and `dd65b5eb`. The "调度器触发前先调 get_pending_realtime_messages_with_limit(1) 检查" patch is one short branch inside `check_realtime_extraction`, but the broader behaviour — what to do when empty — already has a home.

### `f5e50d2d-7c3f-48bc-8d71-1f6bd48c3e72` — `keep`

Heuristic: ask is unsolved; the existing `mission_daemon_update` is *not* the graceful exec replacement the task wants.

- `mission_daemon_update` exists but is blue-green-script oriented:
  - `crates/missiond-daemon/src/handlers/sysinfra/system.rs:15` routes the tool; `:294–306` requires `confirm=true` and the error message points users at "the blue-green deploy script directly for supervised deploys".
  - `:325–336` resolves `binary_dest` from `current_exe()` and `build_target` from `target/release/missiond` — i.e. it builds + copies + restarts; it does not perform an in-process `exec` replacement preserving open MCP socket fds.
- The "graceful restart" surfaces that *do* exist apply to slots, not to the daemon binary itself:
  - `crates/missiond-daemon/src/state.rs:231` "Slots pending graceful restart due to low context (detected 'until auto-compact')".
  - `crates/missiond-daemon/src/supervisor.rs:10, 61` "Threshold: mark for graceful restart when context drops below this %" / "Slot context low, marked for graceful restart on idle".
- No matches for `SIGUSR1`, `exec.*self`, `daemon_upgrade`, or `graceful.*restart` at the daemon-binary level. The slot-driven workaround the task notes ("nohup delayed kill+restart") therefore remains the only path.

Recommendation: keep. The right outcome is a single decision: either implement the SIGUSR1 + fd-inheriting `exec` replacement (nginx-style binary upgrade), or codify the blue-green script as the official answer and add a daemon-side note that `mission_daemon_update` is *only* safe when no MCP-driven slot is the caller. Per dispatch instruction, no sub-task split.

## Notes

- Two of the five (`c8c0345a`, `d1ffe953`) are the same memory-pipeline waste pattern reported under yet another framing. The cleanup wave should auto-collapse rows whose KB refs name `memory-worker-self-referential-feedback-loop`, `memory-worker-empty-batch-rate-persistent`, etc.
- `57304f76` joins `233c10f9` and `8eff0df1` as misfiled HostVDS / xjp-mcp infra rows.
- `e988ada0` and `f5e50d2d` are clean keepers — both name a single concrete location and a small change.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-19-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ `must_not_touch` directories (`.git`, `crates/`, `packages/`, `scripts/`) untouched (read-only).
- ✅ Each reviewed task carries one classification from the allowed set and at least one of `file_path:line` / skill-file reference / project-memory line / cross-batch fact as evidence.
- ✅ Heuristic format honoured: original-ask validity question first, then SSOT/code/runtime check, then a single actionable verdict per task — no sub-task spinning.
- ✅ Final answer follows the Findings / Evidence / Recommendations / Verification contract; no raw KB JSON or full logs pasted.
