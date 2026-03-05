//! Centralized LLM prompt constants.
//!
//! Phase 3 PR5: Collects static prompts from decision_engine and flow_engine
//! into a single module for maintainability and grep-ability.
//!
//! NOTE: Flow phase templates (Investigate/Plan/Execute/Finalize) remain in
//! their builder function because they're tightly coupled to format!() logic.

/// Decision engine prompts (3-tier question routing).
pub(crate) mod decision {
    /// Tier 2 system prompt: instructs Gemini to output strict JSON decision.
    pub const TIER2_SYSTEM: &str = "\
你是一个高级架构决策大脑。请基于提供的上下文，对当前遭遇的困境做出决断。\
你必须且只能输出严格的 JSON 格式。\
如果你可以直接做出决定，请输出：{\"action\": \"DECIDE\", \"decision\": \"你的具体决策\", \"reasoning\": \"简要理由\"}。\
如果该问题需要动态运行代码、查看报错日志或全局搜索未知的代码库才能决定，你必须弃权并输出：{\"action\": \"ESCALATE_TIER_3\", \"decision\": \"\", \"reasoning\": \"需要阅读源码\"}。";

    /// Tier 3 opening line: highest-priority diagnostic dispatch.
    pub const TIER3_HEADER: &str = "【最高优先任务】主干线工位在执行任务时遇到了阻塞。";

    /// Tier 3 closing template: `{0}` = question.id
    pub const TIER3_FOOTER: &str = "\n\
你的唯一目标是：通过阅读代码、运行测试等方式，查明真相并给出明确的决策。\n\
**警告**：你是只读/诊断性质的工位，禁止进行任何实质性的业务代码修改。\n\
当你得出结论后，请立刻调用 `mission_question_answer(id=\"{0}\", answer=\"你的结论\")` 提交结果。";

    /// Decision harvest prompt template: `{0}` = task_title, `{1}` = qa_text
    pub const HARVEST_TEMPLATE: &str = "\
以下是刚成功完成的任务「{0}」中的具体技术决策（均已通过代码落地验证）。\n\
请将每条决策提炼为一条普适的架构/决策规则。\n\n\
**要求：**\n\
1. 剥离具体变量名、版本号、文件路径，保留问题特征和决策原则\n\
2. 每条规则输出为 JSON 对象，包含 key、summary、detail\n\
3. key：动宾结构连字符短语（如 resolve-dependency-conflict-legacy）\n\
4. summary：`[触发条件词簇] → [核心原则] → [动作]`，必须包含触发该问题的所有可能同义词、报错关键字和核心名词\n\
5. detail：JSON 对象 {\"scenario\": \"...\", \"decision\": \"...\", \"reasoning\": \"...\"}\n\
6. 如果多条决策本质相同，合并为一条\n\
7. 只输出 JSON 数组，不要包含任何其他文本\n\n\
**Few-Shot 示例（必须严格遵循此格式）：**\n\
错误示范 summary：'遇到依赖冲突时选择兼容方案。'\n\
正确示范 summary：'遇到 第三方 API 接口 废弃 deprecated 过期 报错 不兼容 时，优先采用 降级 兼容 强行覆盖 旧版本 原则，切勿 大规模重构 业务逻辑'\n\
正确示范完整条目：\n\
{\"key\":\"handle-third-party-api-deprecation\",\"summary\":\"遇到 第三方 API 接口 废弃 deprecated 过期 报错 不兼容 时，优先采用 降级 兼容 强行覆盖 旧版本 原则，切勿 大规模重构 业务逻辑\",\"detail\":{\"scenario\":\"调用外部依赖或第三方 SDK 时发现其 API 已废弃导致编译或运行报错\",\"decision\":\"寻找最小侵入性的兼容方案（如编写 adapter、忽略警告或锁死旧版本）\",\"reasoning\":\"保持主线业务稳定性，避免因外部环境变化引发不可控的级联重构\"}}\n\n\
决策记录：\n{1}";
}

/// Flow engine prompts (engineering task lifecycle).
pub(crate) mod flow {
    /// Decision Engine help protocol injected for all slot phases.
    /// `{task_id}` is the Board task ID for auto-linkage.
    pub const HELP_PROTOCOL: &str = r#"

---
【主控求助协议】
当你遇到**阻断性困境**时，严禁自行盲目尝试超过 3 次或随意猜测架构意图。调用 `mission_question_create(target="master", taskId="{task_id}")` 呼叫主控。
**呼叫条件与 decisionType 映射（严格遵守）：**
1. `architecture`：涉及引入新依赖、修改数据库表、变更核心状态机（必须呼叫）
2. `risk`：发现方案可能破坏现有功能或数据（必须呼叫）
3. `implementation`：有两种可行方案无法权衡（附带 options）
4. `investigation`：遇到不熟悉的黑盒 API（附带已查阅的上下文）
5. `debug`：同一致命报错尝试修复 2 次仍失败（附带报错和尝试记录）

**参数要求：** 必须在 `options` 中提供分析或候选项（如 "A: 修改基类, B: 新增 wrapper"），不能只抛出问题。"#;
}
