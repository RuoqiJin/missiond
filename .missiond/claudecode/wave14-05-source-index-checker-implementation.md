# Wave 14 / Task 05 — implement source-index checker phase 3.1

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

目标：Wave12/13 在 Lisp 蓝图里设计了 source-index / compression checker 规则，但 `scripts/check-architecture-lisp.mjs` 主要还是结构检查。本任务实现最小 checker：检查 source-index entry 的必填字段和 section-id 唯一性。

Ownership：
- `scripts/check-architecture-lisp.mjs`
- `.missiond/v2/architecture-dsl.lisp` 仅限同步 checker-status 文字（如需要）
- `.missiond/v2/intent-pillar-source-index.lisp` 仅限修正 checker 揭示的格式问题

禁止：
- 不改 Rust
- 不改主 Lisp 大段语义
- 不做压缩 / shard split
- 不新增 npm 依赖
- 不要 `git add .`

功能要求：
1. 在 `check-architecture-lisp.mjs --all-v2` 中增加 source-index 检查：
   - section-id 唯一
   - 每个 entry 必须有 `:section-id`
   - 每个 entry 必须有 `:file`
   - 每个 entry 必须有 `:local-path`
   - 每个 entry 必须有 `:status`
2. 如果 entry 有 `:compression-safe?`，值必须是 boolean-ish 或 allowed atom/string：`true|false|yes|no|safe|unsafe|defer`。
3. 输出错误包含 file + line。
4. 不要求 parser 完整理解 Lisp；可以对 `intent-pillar-source-index.lisp` 做保守 scanner。
5. 保持现有 14 files OK。

测试/验收：
- 如脚本已有 test harness，补测试；没有则至少加入内联 dry fixture helper。
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check scripts/check-architecture-lisp.mjs .missiond/v2/architecture-dsl.lisp .missiond/v2/intent-pillar-source-index.lisp`

提交：
- scoped commit，只 stage ownership 文件。
- commit message:
  `chore(v2): enforce source-index checker rules`

交付报告：
- commit hash
- 新增 checker 规则
- 是否修复了 source-index 格式问题

