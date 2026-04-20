我先把第一轮可交付版本做出来了。

最关键的结论是两条。

第一，**“代码真实状态”的判真顺序**我已经帮你定好了：
**memory / event-bus 以新架构压缩包里的专档为准；其余 pillar 以旧地图压缩包为主，再用 v2 `intent.lisp` 做命名与设计哲学校正。**

第二，**多 agent 协同共享执行层已经不该再只是一个技巧**。
我在 `intent-memory-execution.lisp` 里抓到了一个很硬的实战信号：**重复 `D010`**。这说明 execution-log 这套东西已经需要正式协议化，至少要补上：

* 原子 ID allocator
* claim lease / heartbeat / stale-claim 回收
* audit / repair

我做好的文件在这里：

总打包版：
[missiond_pillar_deliverables.zip](sandbox:/mnt/data/missiond_pillar_deliverables.zip)

单文件版：
[真实状态索引：intent-pillar-source-index.lisp](sandbox:/mnt/data/missiond_pillar_deliverables/intent-pillar-source-index.lisp)
[memory 共享执行层更新补丁：intent-memory-agent-execution-update.lisp](sandbox:/mnt/data/missiond_pillar_deliverables/intent-memory-agent-execution-update.lisp)
[worker pillar 细化稿：intent-worker.lisp](sandbox:/mnt/data/missiond_pillar_deliverables/intent-worker.lisp)
[tools pillar 细化稿：intent-tools.lisp](sandbox:/mnt/data/missiond_pillar_deliverables/intent-tools.lisp)
[intent-layer 细化稿：intent-intent-layer.lisp](sandbox:/mnt/data/missiond_pillar_deliverables/intent-intent-layer.lisp)
[system-layer 细化稿：intent-system-layer.lisp](sandbox:/mnt/data/missiond_pillar_deliverables/intent-system-layer.lisp)
[flow pillar 细化稿：intent-flow.lisp](sandbox:/mnt/data/missiond_pillar_deliverables/intent-flow.lisp)
[整合说明：README.md](sandbox:/mnt/data/missiond_pillar_deliverables/README.md)

这轮我实际完成的是：

1. 把“哪个旧图最代表真实代码状态”梳成了一份**判真索引**。
2. 把 **memory 里的 agent-execution-coordination** 按实际施工经验升级成了正式协议补丁。
3. 按你要的结构，给剩下 5 个 pillar 都写了一版 **pillar 级 ingress/core/egress + 功能级 path ingress/logic-core/egress** 的细化稿。
4. 每份稿子都尽量把**旧地图里的真实实现切面**和**你 v2 的设计哲学**接上了。

需要说明的一点是：这轮“真实状态”判断是基于你给的两套 **lisp 地图压缩包**，不是直接扫源码仓库本体。所以这里的“真实状态”是“**以旧地图作为代码代用真相**”来判的；在你当前材料范围内，这是最稳妥的落法。

下一步最自然的是：把这 5 份 pillar 稿下沉到 v2 主 `intent.lisp` 的 `:file` 引用里，再做一轮 drift audit，把 worker footprint、bootstrap count、zombie 文件这些残差一起收口。
