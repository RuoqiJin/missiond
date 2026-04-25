# Wave 12 / Task 06 — Lisp source-index expansion before compression

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

建议派给常驻 Lisp 工位，不要新开无上下文会话。只改 v2 Lisp，不改 Rust/SQL/JS。不要做真正压缩主 Lisp。

目标：为后续 Lisp 语义压缩做准备，扩展 stable section-id/source-index 覆盖面，让未来压缩和拆 shard 有稳定 anchor。

Ownership：
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/architecture-dsl.lisp`
- 如需要，只允许最小更新 `.missiond/v2/intent.lisp` 的 source-index 状态摘要。

不要修改：
- Rust / SQL / JS / Cargo
- 其他 v2 Lisp 主体内容

任务要求：
1. 扩展 source-index 覆盖以下区域：
   - execution coordination / scoped commit handoff
   - file-first artifacts
   - review gate
   - PLAN DAG scheduler
   - methodology compiler / semantic lifting
   - capability usage semantic evidence
   - workstation orchestration
2. 每个 entry 至少包含：
   - `section-id`
   - `file`
   - `local-path`
   - `status`
   - `primary-code-targets`
   - `compression-safe?`
3. 在 architecture-dsl 中补 checker future rule：source-index entry 的 `file/local-path/status` 必填，section-id 唯一。
4. 明确“本任务不压缩主 Lisp、不拆 shard”。

验收：
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/architecture-dsl.lisp .missiond/v2/intent.lisp`

交付：
- scoped commit，只 stage 本任务 ownership 文件。
- commit message 建议：
  `docs(v2): expand source index before compression`
- 报告 commit hash、entry 数量、仍未覆盖区域。

