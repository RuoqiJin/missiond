根据输入材料中的交互历史，以下是严格按照 `conversation-memory-distillation` 规则提取的记忆蒸馏报告：

### 1. 候选记忆（应提升为长期记忆/项目/Universe常量）
*   **开发策略与规范**：Auth 领域已确立 `touched-file-only rustfmt policy`（仅格式化修改过的文件）并记录于 blueprint。
*   **领域模型术语**：Auth 模型已明确锚定 `user-group terminology`。
*   **架构边界与历史例外**：Auth 服务中 `SERVER_PORT` 相关的旧式例外（stale exception）已调和完成，可作为架构演进历史备案。
*   **模型与路由能力边界**：明确记录系统在派发机制上的盲区——曾错误将需要跨项目访问和强依赖 Board/KB 工具的 context-pack 调查任务路由给 `Gemini`（甚至 `gemini-ultra`），导致任务阻塞。Gemini 适用于低权威摘要，不适合承担结构/代码事实的强制性调查。
*   **遥测局限性**：当前存在 `conversations.task_id` 归因缺陷（attribution issue），导致通过 `mission_conversation_query` 依靠 taskId 检索会话证据时返回为空。

### 2. 应丢弃的内容（已被 SSOT 覆盖或视为噪音）
*   **各项目 M6 推进的临时状态**：如 Auth, Infra, PCEA, Jarvis, cuthub-frontend, secret-store-rs 等子系统收敛到 M6 过程中的 Shard A/B/C、Running->Done 的流水线流转事件（已被固化到各自的 SSOT Lisp、Blueprints 及 final-convergence 静态结果中）。
*   **已修复的单次环境报错痕迹**：在 `/private/tmp` 发生的 `ENOSPC` 磁盘满报错及手动删除 MissionD Rust incremental cache 的过程。
*   **误唤醒与心跳噪音**：如 `Done->Done` 的状态重复心跳、由于 projection drift 导致的重复分类评估（classify_objective），以及未导致实质修改的 read-only diagnosis。

### 3. 基础设施待优化项（Infrastructure Issue Inventory）
*   **磁盘空间治理**：`/private/tmp` 的 `ENOSPC` 错误多次发生，甚至直接阻塞了 `INFRA_M6` 和 `AUTH_M6_SHARD_A1` 等任务的执行。当前依赖手动清除 Rust 构建缓存，需要自动化的磁盘监控与空间回收机制。
*   **Swarm 路由与授权缺陷**：存在 Dispatch correction 现象（子任务被错误发送给无权限的 Gemini）。调度层缺少对所需工具集和跨项目读写权限的前置校验。
*   **任务归因与审计数据断层**：由于 worker 执行的 provider durable JSONL 没有被正确挂载 `task_id`，引发 `mission_conversation_query` 失效，这破坏了溯源完整性。
*   **环境挂起风险**：`neural-codegen` 的 M6 SSOT 提交曾被 GPG 签名过程阻塞（commit blocked on GPG），暴露了自动化提交脚本在无交互 TTY 环境下的稳定性隐患。
*   **状态投影漂移（Projection Drift）**：Master-control 中出现相同的 Slot 被多重 claim (`STALE_SAME_SLOT_CLAIM`)，以及 Lisp checkpoint 状态落后于实际 Board 任务状态的现象，引发了无意义的重试。

### 4. 需用户决策的不确定项
*   **域名一致性**：Auth KB 评估报告指出存在 `.com` 与 `.top` 域名相关的规范选择，需用户定夺最终权威域名。
*   **旧知识条目处置**：Auth 知识库中遗留的 `6a4a7a48` 条目是否应拆分，以及 `f6589b56` 条目是直接删除还是标记为历史（historical）。
*   **Jarvis-mechanic 激活决策**：当前 `jarvis-mechanic` 在 Registry 中处于 `active=false` 且 `intent_path=null` 状态，阻碍了该工具链的进一步收敛。需用户决定是否激活该项目并补充缺失的 intent_path。

### 5. 建议下一批让 ClaudeCode 执行的机械清理任务
*   等待用户对域名做出决策后，批量执行跨仓库的 `.com` / `.top` 域名字符串替换与对齐。
*   根据用户对 Auth KB 的裁决，自动执行 markdown 知识库重构：分割 `6a4a7a48` 相关的文本，并将 `f6589b56` 归档。
*   批量修复/回填已知成功完成但缺少 `taskId` 归因映射的近期 provider 历史数据（修补 audit trail）。
