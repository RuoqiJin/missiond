# MissionD pillar deliverables

这批文件做了三件事：

1. `intent-pillar-source-index.lisp`
   用来回答“哪个旧地图文件最能代表代码真实状态”。

2. `intent-memory-agent-execution-update.lisp`
   把多 agent 协同共享执行层的真实发现回灌到 memory pillar 设计里。
   这里最关键的更新是：
   - execution-log 从单点 pilot 升级为正式协议
   - 引入 `id-counters`
   - claim 增加 `lease + heartbeat + stale reap`
   - 增加 `audit / repair` 两个运维动作

3. `intent-worker.lisp` / `intent-tools.lisp` / `intent-intent-layer.lisp` / `intent-system-layer.lisp` / `intent-flow.lisp`
   这 5 份是按你现在的设计哲学重写出的剩余 pillar 细化稿，统一采用：
   - pillar 级：`ingress / core / egress`
   - 功能级：`path -> ingress / logic-core(step s1..sn) / egress`

## 建议并入顺序

1. 先把 `intent-pillar-source-index.lisp` 当作“判真索引”保存下来。
2. 再把 `intent-memory-agent-execution-update.lisp` 中的 helper patch 合并进 `intent-memory.lisp`。
3. 然后把 5 个剩余 pillar 文件各自挂到 v2 `intent.lisp`，让总纲只保留摘要，细节下沉到分 pillar 文件。
4. 最后开一轮 drift audit，把旧地图里已经过时的 worker footprint / bootstrap count / dead files 再扫一次。

## 我这次特意收进设计里的真实发现

- `intent-memory-execution.lisp` 里已经出现重复 `D010`，说明 execution 共享层不能继续依赖手工分配 ID。
- `memory` 和 `event-bus` 都已经在用 execution-log 配对模式，所以它已经不是一次性技巧，而是 pillar 级治理能力。
- 剩余 pillar 的“代码真实状态”仍主要藏在旧地图压缩包里；新 v2 `intent.lisp` 更适合作为命名、边界和哲学总纲。
