# MissionD wave 11 Lisp design: source-index before compression

使用常驻 Lisp 架构会话执行。可以使用 agent-team 提高效率，但最终由一个主 agent 统一落笔。只改 `.missiond/v2` 下 architecture/index Lisp，不改 Rust/SQL/JS，不 stage，不 commit。

## 目标

不要压缩主 Lisp 正文。先做压缩前置设施：稳定 source index / section id / target-path / implementation-target taxonomy。这样之后真正拆分/压缩时，不会丢 cross-ref。

## 写入范围

优先修改：

- `.missiond/v2/architecture-dsl.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`

如确实需要，可小幅修改：

- `.missiond/v2/intent.lisp` 仅增加 source-index cross-ref，不改各 pillar 正文。

不要修改：

- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- Rust / SQL / JS

## 设计要求

补一个可复用 DSL/index 约定，至少覆盖：

- `section-id`: stable id, 不随标题文案变化。
- `pillar`: flow/tools/memory/worker/intent-layer/event-bus/system-layer。
- `source-file`: 当前 Lisp 文件。
- `line-anchor-policy`: 不依赖固定行号，依赖 section id + local path。
- `implements`: code target paths list。
- `status-taxonomy`: architecture-designed / code-aligned / code-aligned partial / operational-practice / pending / deprecated / protected。
- `split-policy`: 何时可从主文件拆到 shard。
- `compression-policy`: 只压缩重复状态文本，不压缩 ingress/logic-core/egress 的执行步骤。

## 需要显式记录的判断

- 现在不做主 Lisp 压缩。
- 先建 index 和 checker 可读约定。
- 等 file-first writer + review gate + PLAN DAG 最小闭环稳定后，再做正文压缩/拆分。

## 验收

- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check .missiond/v2/architecture-dsl.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent.lisp`

## 交付报告

说明：

- 新增 DSL forms。
- 后续拆分规则。
- 为什么暂不压缩主文件。
- 未触碰主大文件确认。

