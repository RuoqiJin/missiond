# MissionD Board Cleanup Batch 06

- generated_at: 2026-05-10
- scope: `project=missiond`, `status in (open, blocked)`, high-priority window offset 25
- mode: read-only investigation; no Board mutation

## Batch conclusion

This batch contains three mostly-covered historical runtime bugs and two router/runtime reliability items that should be rewritten into current SSOT surfaces instead of kept as old standalone tasks.

Recommended close/merge actions:

1. Close the AIOps MCP flood task as covered by current incident dedupe/cooldown.
2. Merge the memory extraction concurrency/self-reference tasks into the existing `memory-extraction-anti-spin-policy` and `role-stable-worker-lanes` rewrite candidates.
3. Rewrite the router timeout / engineering-flow tasks into one current task: `router-runtime-timeout-and-review-gate-projection`.

## 1. `8e8ed2d5-1c62-4ff7-974f-740440dbe5cf`

- title: `[Bug] Board 自愈告警洪泛：120+ 条 "[自愈] MCP 工具不可用: Bash (slot-memory-slow)" 重复任务`
- classification: `close-covered`
- current status: The old flood path is covered by provider-side cooldown plus AIOps incident dedupe. The remaining lesson belongs in incident-governance checks, not an open remediation task.
- evidence:
  - `crates/missiond-daemon/src/workers/local/pty_event_worker.rs` defines `MCP_ERROR_COOLDOWN` keyed by `slot_id:tool_name`.
  - `crates/missiond-daemon/src/infra/aiops.rs` computes `build_dedupe_key(source, title)` and uses `find_open_task_by_dedupe_key` before creating new remediation tasks.
  - `create_pty_remediation_task` receives the same dedupe key and appends notes to an existing task for repeated incidents.

## 2. `4569959d-10bc-49f5-ac3a-31f9f87e7af2`

- title: `Router Chat 可靠性 + Engineering Flow Gate 检查`
- classification: `rewrite-candidate`
- proposed rewrite: `router-runtime-timeout-and-review-gate-projection`
- current status: Router Chat reliability has been partially rebuilt, but the old “Engineering Flow Gate” wording is stale. The current equivalent should be expressed as review-gate / exact-shard acceptance / router runtime timeout projection.
- evidence:
  - `crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs` loads `RouterRuntimeConfig`, applies token budgets, persists task-scoped chat history, and accepts an optional `idle_timeout`.
  - `crates/missiond-daemon/src/handlers/comm/router_chat/manage.rs` owns history/list/delete/restore/compress surfaces.
  - `scripts/check-v3-router-policy-isomorphism.mjs` pins the router-policy implementation map and runtime projection.
  - No current strict `APPROVED/REJECTED` text gate was found; current control lives in plan review gates, exact shard contracts, and acceptance evaluators.

## 3. `65941493-444e-4bdf-87c9-0f2272db5c12`

- title: `修复 realtime-extract 双 worker 并发竞争：加 active_worker_lock`
- classification: `merge-into-existing-candidate`
- merge target: `memory-extraction-anti-spin-policy`
- current status: The specific active-worker-lock shape has been superseded by a single extraction lane, watermarks, and artifact-based memory review. Keep only as evidence for the anti-spin / single-lane invariant.
- evidence:
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` has `try_claim_extraction_probe` and a single extraction state path.
  - The same file filters no-user sessions and advances watermarks on completion/error/timeout.
  - `crates/missiond-daemon/src/handlers/compute/task_delegate.rs` now has stronger slot reservation and duplicate code-worker guards for delegated workers.

## 4. `53318288-0a62-4412-8631-80516f8f35ec`

- title: `修复 Memory Worker 自引用反馈循环...`
- classification: `merge-into-existing-candidate`
- merge target: `memory-extraction-anti-spin-policy`
- current status: Duplicate of the Batch 5 memory self-reference cluster. The main SQL path now scopes realtime extraction to user conversations, but importer/exporter regression tests should remain part of the merged candidate.
- evidence:
  - `crates/missiond-core/src/db/pg/conversation.rs` realtime pending queries use `c.conversation_type = 'user'`.
  - Batch 5 found the same issue family in `97595bc3`, `e7d4e337`, `43292726`, and `2ba85d68`.

## 5. `866ebc22-eb35-4ef5-a003-f399aa344ba2`

- title: `router_chat 默认 idle_timeout 改为 600s`
- classification: `rewrite-candidate`
- proposed rewrite: `router-runtime-timeout-and-review-gate-projection`
- current status: Not safely closable. The V3 router runtime owns several Gemini timeouts, but `mission_router_chat` still exposes a 120s MCP schema default and, when the caller omits `idle_timeout`, falls through to `GeminiCliConfig.timeout` from `llm.yaml`.
- evidence:
  - `crates/missiond-mcp/src/tools/comm/router_chat.rs` still declares `"idle_timeout": ... "default": 120`.
  - `crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs` maps caller `idle_timeout` to `Option<Duration>` and passes `None` when omitted.
  - `crates/missiond-daemon/src/main.rs` constructs `GeminiCli::new(... Duration::from_secs(cli_cfg.timeout) ...)`, so the implicit idle timeout is provider config, not clearly V3-projected 600s.
  - `.missiond/v3/missiond-blueprint.lisp` already states router runtime policy owns router/Gemini timeout projection, so the fix should be a small SSOT/runtime alignment task rather than this old standalone bug.
