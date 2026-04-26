# Wave 13 / Task 04 — Lisp backfill for Wave13 implementation status

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

建议派给常驻 Lisp 工位。只改 v2 Lisp，不改 Rust/SQL/JS。不要做大压缩。

前置：等 Wave13 Task 01/02/03 至少一项完成并 commit 后再执行；如果只完成其中一项，就只回填对应状态。

目标：把 Wave13 code-alignment 的真实状态回填到 Lisp 蓝图和 source-index。保持高信息密度，但不要进行主文件压缩或 shard 拆分。

Ownership：
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/intent.lisp`

禁止：
- 不改 Rust/SQL/JS/Cargo
- 不改 `.missiond/intent-mcp-defs.lisp`
- 不做 L2/L3 压缩，不拆 shard

回填要点：
1. Evidence collector integration：
   - plan evidence sidecar 从 legacy builder 升级为 typed collector 的位置与剩余 pending。
2. PLAN DAG runtime v2：
   - max_parallel_nodes / node lifecycle / failure-policy / evidence transition。
3. Unified entry pipeline v0：
   - 是否新增 tool；如果没有新增，写明复用现有 manager surfaces。
4. 更新 source-index entries：
   - section-id
   - status
   - primary-code-targets
   - compression-safe?
5. 保留 pending：
   - auto approve / autonomous review answer
   - full semantic PLAN interpretation
   - UI review panel
   - event bus live subscription if still missing

验收：
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-memory.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-worker.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent.lisp`

提交要求：
- scoped commit，只 stage ownership 文件。
- commit message 建议：
  `docs(v2): backfill wave13 execution status`

交付报告：
- commit hash
- 修改文件
- 状态升级列表
- 仍 pending 列表

