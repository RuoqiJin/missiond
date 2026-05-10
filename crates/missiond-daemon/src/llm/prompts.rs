//! Centralized LLM prompt store with file-based hot-reload.
//!
//! Phase 3 PR5: Collects static prompts from decision_engine and flow_engine.
//! Phase 4 PR2: Added PromptStore with file override from ~/.xjp-mission/prompts/.
//!
//! At startup, each prompt checks for a file override at:
//!   ~/.xjp-mission/prompts/{key}.txt
//! If found, uses file content; otherwise falls back to compiled-in default.
//! `reload()` re-reads all files without daemon restart.

use std::sync::RwLock;
use tracing::{debug, info};

/// Default prompt values (compiled-in).
mod defaults {
    pub const TIER2_SYSTEM: &str = "\
你是一个高级架构决策大脑。请基于提供的上下文，对当前遭遇的困境做出决断。\
你必须且只能输出严格的 JSON 格式。\
如果你可以直接做出决定，请输出：{\"action\": \"DECIDE\", \"decision\": \"你的具体决策\", \"reasoning\": \"简要理由\"}。\
如果该问题需要动态运行代码、查看报错日志或全局搜索未知的代码库才能决定，你必须弃权并输出：{\"action\": \"ESCALATE_TIER_3\", \"decision\": \"\", \"reasoning\": \"需要阅读源码\"}。";

    pub const TIER3_HEADER: &str = "【最高优先任务】主干线工位在执行任务时遇到了阻塞。";

    pub const TIER3_FOOTER: &str = "\n\
你的唯一目标是：通过阅读代码、运行测试等方式，查明真相并给出明确的决策。\n\
**警告**：你是只读/诊断性质的工位，禁止进行任何实质性的业务代码修改。\n\
当你得出结论后，请立刻调用 `mission_question_answer(id=\"{0}\", answer=\"你的结论\")` 提交结果。";

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

    pub const EXTRACTION_REALTIME: &str = "\
有新的对话内容待分析。

📋 工作流程:
1. 调用 mission_memory_pending 获取待分析内容（⚠️ 只调用一次；上下文压缩后可由系统重放同一批次，超限才返回结构化 MEMORY_PENDING_ALREADY_SERVED 错误）
2. 用 mission_kb_search 去重检查
3. 用 mission_kb_remember 存入新知识
4. 发现 bug → mission_board_create 上报
5. 处理完毕后输出简短总结即可结束（如'提取了 2 条知识'或'无新知识'）

⚠️ 水位线由系统自动管理，不需要调用任何 done/finish/complete 工具。你的文本回复即代表本轮处理结束。
🚫 不要主动轮询 mission_memory_pending — 水位线在本轮结束后才推进；只有上下文丢失/压缩后才依赖系统重放同一批次。

⚠️ 异常处理（重要）:
如果 MCP 工具调用失败、超时或不可用:
- 不要尝试用 Bash/sqlite3 等替代方案访问数据库
- 不要自行查找或修改文件系统中的 .db 文件
- 直接输出: <slot_anomaly type=\"mcp_unavailable\" tool=\"工具名\" error=\"错误描述\"/>
- 然后停止工作，等待 orchestrator 恢复
orchestrator 会自动检测并处理 MCP 连接问题，你只需上报即可。

📝 本工位职责:
- 数据来源: 仅 mission_memory_pending（跨会话分析归 deep-analysis 工位负责）
- 所有数据读写通过 MCP 工具完成，不直接访问文件系统中的数据库

🎯 提取目标（按优先级）:
- 用户偏好/原则/纠正 → category: preference
- 架构决策/技术事实 → category: memory 或 memory:architecture/memory:decision
- 已修 bug 根因（仅最终结论） → category: memory:bugfix
- 运维痛点信号 → category: memory:ops
- 调试弯路经验（仅泛化路径） → category: memory:debug

🚫 严禁提取（违规内容会被系统拦截拒写）:
- 具体代码排查过程、报错堆栈、单次 debug 流水账
- 基础设施信息/API 细节/版本号/通用技术知识/当天工作日志
- 对话仍在排查/试错/阅读日志中（未确认解决）→ 禁止写 memory:bugfix 和 memory:debug，留给 deep-analysis
去重: 提取前 mission_kb_search 检查。

📐 写入格式强制要求:
summary ≤ 120 字，必须是结论性陈述，禁止叙事体
detail 必须遵循三段式: {\"trigger\": \"触发条件\", \"conclusion\": \"最终结论\", \"action\": \"应采取的动作\"}
如果知识关联特定代码符号（函数/结构体/模块），可选添加: \"symbol\": \"符号名\", \"file_hint\": \"文件路径\"
系统会自动将知识与 AST 代码节点建立图谱链接，提升代码上下文注入精度。

❌ Bad Case（系统会拒绝的写法）:
summary: '先查看了 xxx.rs 的第 30 行，发现 parse 报错 InvalidToken，然后尝试改用 serde_json，但还是失败，最后发现是 UTF-8 BOM 导致...'
❌ 这是流水账，不是知识。

✅ Good Case（正确写法）:
summary: 'JSON 解析 InvalidToken 报错时，优先检查文件头 UTF-8 BOM 而非换解析库'
detail: {\"trigger\": \"serde_json parse 报 InvalidToken，换库无效\", \"conclusion\": \"根因是文件头 3 字节 UTF-8 BOM\", \"action\": \"用 BufReader skip_bom 或 strip_prefix\"}";

    pub const EXTRACTION_HABITS: &str = "\
你是用户行为分析专家。请分析以下历史对话，提取用户持久的【操作习惯和偏好】。
不要提取特定项目的业务逻辑，只提取可跨项目泛化的行为模式。特别关注用户的【纠正】、【批评】或【明确指令】。

📋 工作流程:
1. 调用 mission_conversation_get 获取指定会话的消息内容
2. 分析用户消息中的行为模式（重点关注命令式指令、否定句、重复要求）
3. 用 mission_kb_search 去重检查（避免与现有 preference 重复）
4. 用 mission_kb_remember 存入新发现的习惯

⚠️ 水位线由系统自动管理。你的文本回复即代表本轮处理结束。

🎯 提取维度（4 类）:
1. workflow — 工作流习惯（如：先调查再修改、方案需 Gemini 审阅后执行、复杂任务先建 Board）
2. style — 沟通/代码风格偏好（如：要求简洁回复、偏好中英混用、不要总结已完成的工作）
3. technical — 技术约束/偏好（如：禁用某框架、优先某工具、特定命名规范）
4. correction — 纠错触发器（如：AI 做了 X 导致用户不满并要求重做的模式）

📐 写入格式:
category: preference（所有习惯统一存 preference 类别，key 体现子类型）
key 命名: habit-{workflow|style|technical|correction}-简短描述
summary ≤ 120 字，必须包含用户原话作为证据
detail: {\"dimension\": \"workflow|style|technical|correction\", \"pattern\": \"习惯描述\", \"trigger\": \"触发场景\", \"user_quote\": \"用户原话\"}

✅ Good Case:
key: habit-workflow-gemini-review-before-execute
summary: '调查方案需经 Gemini 审阅通过后才能执行 — 用户原话: \"调查后必须经 Gemini 审核优化方案后才执行\"'
detail: {\"dimension\": \"workflow\", \"pattern\": \"方案设计后必须发 Gemini 审阅，通过后才执行\", \"trigger\": \"AI 完成调查/设计方案时\", \"user_quote\": \"调查后必须经 Gemini 审核优化方案后才执行\"}

🚫 严禁:
- 提取具体代码/API/版本信息
- 提取单次偶然行为（需要在对话中出现明确的指令或纠正才算习惯）
- 提取通用技术知识";

    pub const EXTRACTION_DEEP: &str = "\
⚠️ 重要: 消息级知识（偏好/决策/事实）已由 realtime 管道提取，不要重复提取。
你的任务仅限于:
1. 跨会话模式 — 用 mission_conversation_search 搜索相关会话，发现反复出现的主题
2. 工作流抽象 — 可以固化为工具/服务的重复操作
3. 知识关联 — 不同会话之间的隐含联系
4. 趋势发现 — 用户行为/需求的演变方向
5. 问题上报 — 发现 bug/资源浪费/反复出错 → mission_board_create 创建任务
6. 运维链路审计 — 重复的多步手动操作（SSH→查日志→重启→再查）→ 封装为 MCP 工具建议，存 category: memory:ops
7. 调试经验提炼 — 只提炼「正确排查路径」（3 步以内），禁止记录试错过程。存 category: memory:debug
8. 架构决策模式 (policy:decision) — 用户面对技术选项时的规律性偏好。\
   必须提炼为泛化规则（剥离具体变量名/版本号），而非单次操作记录。\
   summary 格式：[触发条件词簇] → [核心原则] → [动作]，富含可能出现在提问中的名词。存 category: policy:decision

🚫 严禁提取（违规内容会被系统拦截拒写）:
- 单条消息的偏好/决策/事实（realtime 已处理）
- 当天工作日志、版本细节
- 调试过程流水账（每一步尝试了什么、报了什么错）
- 绝对禁止写入 category: infra（基础设施由 servers.yaml 管理）

📐 写入格式强制要求:
summary ≤ 120 字，必须是泛化结论，禁止叙事体
detail 采用三段式: {\"trigger\": \"...\", \"conclusion\": \"...\", \"action\": \"...\"}

❌ Bad Case:
'在排查 deploy-agent 更新失败时，先 SSH 到服务器查看 systemctl status，发现 OOM，然后查看 journalctl -u deploy-agent --since today 发现内存从 200MB 涨到 2GB...'
❌ 这是调试日记，不是知识。

✅ Good Case:
summary: 'deploy-agent OOM 排查：优先看 journalctl 内存趋势而非 systemctl 状态码'
detail: {\"trigger\": \"deploy-agent 更新后服务异常\", \"conclusion\": \"根因是请求体未限流导致内存泄漏\", \"action\": \"加 body size limit + 内存监控告警\"}";

    pub const EXTRACTION_BOARD_PROGRESS: &str = "\
你正在为 Board 任务提取进展报告。请分析以下会话摘要，输出严格 JSON。

输出格式（严格 JSON，无 markdown）:
{
  \"task_progress\": [
    {
      \"task_id\": \"完整任务ID\",
      \"summary\": \"<=300字进展摘要：本次完成了什么、涉及哪些文件/模块、未完成的部分\",
      \"is_done\": false,
      \"confidence\": 0.8
    }
  ]
}

规则:
- is_done=true 仅当用户明确表示完成/验证通过时设置
- confidence < 0.5 时跳过该任务（信息不足）
- 宁可 is_done=false 也不要误标完成
- summary 要具体：提到文件名、函数名、配置项等";
}

/// Runtime prompt data — loaded from files with const fallbacks.
struct PromptData {
    tier2_system: String,
    tier3_header: String,
    tier3_footer: String,
    harvest_template: String,
    help_protocol: String,
    extraction_realtime: String,
    extraction_deep: String,
    extraction_habits: String,
    extraction_board_progress: String,
}

impl PromptData {
    fn load() -> Self {
        let dir = crate::helpers::default_mission_home().join("prompts");
        Self {
            tier2_system: load_or_default(&dir, "tier2_system", defaults::TIER2_SYSTEM),
            tier3_header: load_or_default(&dir, "tier3_header", defaults::TIER3_HEADER),
            tier3_footer: load_or_default(&dir, "tier3_footer", defaults::TIER3_FOOTER),
            harvest_template: load_or_default(&dir, "harvest_template", defaults::HARVEST_TEMPLATE),
            help_protocol: load_or_default(&dir, "help_protocol", defaults::HELP_PROTOCOL),
            extraction_realtime: load_or_default(
                &dir,
                "extraction_realtime",
                defaults::EXTRACTION_REALTIME,
            ),
            extraction_deep: load_or_default(&dir, "extraction_deep", defaults::EXTRACTION_DEEP),
            extraction_habits: load_or_default(
                &dir,
                "extraction_habits",
                defaults::EXTRACTION_HABITS,
            ),
            extraction_board_progress: load_or_default(
                &dir,
                "extraction_board_progress",
                defaults::EXTRACTION_BOARD_PROGRESS,
            ),
        }
    }
}

fn load_or_default(dir: &std::path::Path, key: &str, default: &str) -> String {
    let path = dir.join(format!("{}.txt", key));
    match std::fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => {
            info!(key, path = %path.display(), "Prompt loaded from file override");
            content
        }
        _ => default.to_string(),
    }
}

/// Thread-safe prompt store with hot-reload support.
pub(crate) struct PromptStore {
    data: RwLock<PromptData>,
}

impl PromptStore {
    /// Load prompts from files (with const fallbacks).
    pub fn load() -> Self {
        Self {
            data: RwLock::new(PromptData::load()),
        }
    }

    /// Re-read all prompt files. Call periodically from autopilot_tick.
    pub fn reload(&self) {
        match self.data.write() {
            Ok(mut d) => {
                *d = PromptData::load();
                debug!("Prompts reloaded");
            }
            Err(e) => tracing::warn!(error = %e, "Failed to reload prompts (poisoned lock)"),
        }
    }

    // Accessor methods — all return owned Strings (cheap, prompts are small)
    pub fn tier2_system(&self) -> String {
        self.data
            .read()
            .map(|d| d.tier2_system.clone())
            .unwrap_or_else(|_| defaults::TIER2_SYSTEM.to_string())
    }
    pub fn tier3_header(&self) -> String {
        self.data
            .read()
            .map(|d| d.tier3_header.clone())
            .unwrap_or_else(|_| defaults::TIER3_HEADER.to_string())
    }
    pub fn tier3_footer(&self) -> String {
        self.data
            .read()
            .map(|d| d.tier3_footer.clone())
            .unwrap_or_else(|_| defaults::TIER3_FOOTER.to_string())
    }
    pub fn harvest_template(&self) -> String {
        self.data
            .read()
            .map(|d| d.harvest_template.clone())
            .unwrap_or_else(|_| defaults::HARVEST_TEMPLATE.to_string())
    }
    pub fn help_protocol(&self) -> String {
        self.data
            .read()
            .map(|d| d.help_protocol.clone())
            .unwrap_or_else(|_| defaults::HELP_PROTOCOL.to_string())
    }
    pub fn extraction_realtime(&self) -> String {
        self.data
            .read()
            .map(|d| d.extraction_realtime.clone())
            .unwrap_or_else(|_| defaults::EXTRACTION_REALTIME.to_string())
    }
    pub fn extraction_deep(&self) -> String {
        self.data
            .read()
            .map(|d| d.extraction_deep.clone())
            .unwrap_or_else(|_| defaults::EXTRACTION_DEEP.to_string())
    }
    pub fn extraction_habits(&self) -> String {
        self.data
            .read()
            .map(|d| d.extraction_habits.clone())
            .unwrap_or_else(|_| defaults::EXTRACTION_HABITS.to_string())
    }
    pub fn extraction_board_progress(&self) -> String {
        self.data
            .read()
            .map(|d| d.extraction_board_progress.clone())
            .unwrap_or_else(|_| defaults::EXTRACTION_BOARD_PROGRESS.to_string())
    }
}
