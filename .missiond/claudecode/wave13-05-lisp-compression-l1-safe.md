# Wave 13 / Task 05 — Lisp L1 safe compression, no shard split

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

建议派给常驻 Lisp 工位。只改 v2 Lisp，不改 Rust/SQL/JS。这个任务只做 L1 安全压缩，不拆 shard。

前置：
- Wave13 Task 04 已回填最新状态并 commit。
- source-index 覆盖已足够，checker 通过。

目标：压缩重复状态长句和冗余 wave 历史描述，保留所有 contract / ingress / logic-core / egress / target path。Lisp 的价值是高信息密度，本任务只做“不损失定位能力”的 L1 压缩。

Ownership：
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent.lisp`

禁止：
- 不改 `intent-pillar-source-index.lisp`，除非只是修引用错误
- 不改 `architecture-dsl.lisp`，除非 checker 要求
- 不拆 shard
- 不删除任何 `:implementation-targets / :primary-code-targets / :flow-ref / :tool / :action / :fields`

允许压缩：
1. 重复的 status 长句，改为 taxonomy + source-index 引用。
2. wave 历史里已由 commit hash/source-index 覆盖的冗长描述，压成单句。
3. 重复 pending 列表合并为 shared pending block。
4. 中文解释性长句改为 compact key/value。

必须保留：
- 所有 named flow/tool/helper/actor 名称
- 所有 file path / module path
- 所有 action enum / field schema / status taxonomy
- 所有 safety invariant / anti-pattern

验收：
- 先记录 `wc -l` 六个文件压缩前后行数。
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`
- 抽样 `rg` 确认关键 anchor 仍存在：
  - `F-scoped-commit-handoff`
  - `plan-dag-scheduler`
  - `evidence-collector`
  - `file-first-artifacts`
  - `workstation-dispatch-policy`
  - `mission_execution`

提交要求：
- scoped commit，只 stage ownership 文件。
- commit message 建议：
  `docs(v2): apply L1 semantic compression`

交付报告：
- commit hash
- 每文件压缩前后行数
- 删除/合并的重复主题
- 明确没有做 shard split

