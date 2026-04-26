# Wave 14 / Task 07 — Lisp backfill + L2 shard split plan

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

建议派给常驻 Lisp 工位。只改 v2 Lisp，不改 Rust/SQL/JS。不要真正拆 shard。

前置：等 Wave14 Task 01/02/03/04 至少一项完成并 commit 后执行；只回填已完成事实。

目标：把 Wave14 的真实实现状态回填到 Lisp，并设计 L2 shard split plan。注意：本任务只设计拆分计划，不移动主 Lisp 内容。

Ownership：
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/intent.lisp`
- `.missiond/v2/architecture-dsl.lisp` 仅限 L2 split policy

禁止：
- 不改 Rust/SQL/JS/Cargo
- 不改 `.missiond/intent-mcp-defs.lisp`
- 不移动大段内容到新 shard
- 不做 L2 实际拆分

回填要点：
1. file-first writer integration 状态：
   - directive alignment artifact
   - PLAN.lisp artifact
   - workflow methodology artifact
2. PlanNodeStateChanged / live EventRef 状态（如果 Task 02 已完成）。
3. review_gate auto-create v1 状态（如果 Task 03 已完成）。
4. unified-entry v1 状态（如果 Task 04 已完成）。
5. source-index checker implementation 状态（如果 Task 05 已完成）。

L2 shard split plan：
1. 提出 shard 候选，但不执行：
   - `intent-execution-governance.lisp`
   - `intent-directive-artifacts.lisp`
   - `intent-plan-dag.lisp`
   - `intent-capability-governance.lisp`
   - `intent-workstation-policy.lisp`
2. 每个 shard 说明：
   - moved sections
   - retained anchor in parent file
   - source-index update rule
   - checker requirement
   - rollback plan
3. 明确 L2 执行 gate：
   - source-index checker passing
   - file-first writer stable
   - review gate stable
   - no active parallel code wave

验收：
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-memory.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-worker.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent.lisp .missiond/v2/architecture-dsl.lisp`

提交：
- scoped commit，只 stage ownership 文件。
- commit message:
  `docs(v2): backfill wave14 status and plan L2 shards`

交付报告：
- commit hash
- status upgrades
- L2 shard plan summary
- explicit statement: no shard content moved

