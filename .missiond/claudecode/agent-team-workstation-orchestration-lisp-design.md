# MissionD v2 Lisp 架构任务：沉淀 ClaudeCode agent-team / 常驻工位 / spawn 策略

工作目录：`/Users/jinchen/Projects/missiond`

你是 ClaudeCode。请只做 Lisp 架构设计，不做 Rust 代码同构。

执行时可以使用 agent-team 提高效率：让子 agent 分别读 worker / flow / intent-layer / tools 的相关区域，但最终由一个主 agent 统一落笔，避免多人同时改同一 Lisp 块造成结构漂移。

## 背景

用户刚补充了几条真实操作经验，这些经验需要沉淀进 MissionD 的架构设计：

1. 给 ClaudeCode 发任务时，如果指令里写“使用 agent-team 提高效率”，ClaudeCode 会并行开几个子 agent，复杂 Lisp/代码同构任务会更快。
2. 长期保持一个“改 Lisp 的常驻 ClaudeCode 会话”很有价值，因为它保留上下文，下次改 Lisp 时能更快定位结构和风格。
3. 代码同构任务适合新开 ClaudeCode 会话，因为任务 `.md` 已经自包含，不需要继承旧上下文，隔离度更好。
4. MissionD 后续应优先使用 spawn 工位 / resident workstation 的形式调度 ClaudeCode，尽量少用 `claude -p` 一次性 prompt 模式。
5. 这些不是临时使用技巧，而是应该成为 MissionD plan-runner / worker orchestration 的默认调度策略。

当前 unified-entry pipeline 已经设计为：

```text
message
  -> intent-alignment.lisp
  -> review
  -> PLAN.lisp
  -> review
  -> MissionD internal execution
  -> evidence
  -> workflow.lisp
```

现在需要把“选择哪种 ClaudeCode 工位/会话执行”的策略补到这条 pipeline 里。

## 只允许修改

优先修改：

- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent.lisp`

如确有必要，可少量同步：

- `.missiond/v2/intent-memory.lisp`

不要修改 Rust / SQL / JS / shell / `.missiond/intent-mcp-defs.lisp`。
不要 stage，不要 commit。

## 设计目标

把 ClaudeCode 调度策略设计成 MissionD 的一等架构规则，尤其是：

- 什么时候复用常驻 Lisp 会话。
- 什么时候新开代码同构会话。
- 什么时候提示 ClaudeCode 使用 agent-team。
- 为什么优先 spawn 工位，而不是 `claude -p`。
- 这些策略如何被 unified-entry pipeline 的 `alignment-author` / `plan-compiler` / `plan-runner` / `evidence-collector` 使用。

## 必须补的架构内容

### 1. `.missiond/v2/intent-worker.lisp`

新增或升级一个 worker/orchestration policy section，例如：

- `claudecode-workstation-orchestration`
- `resident-workstation-policy`
- 或放入已有 slot/orchestrator 章节。

必须包含这些策略：

1. `resident-lisp-architect-session`
   - 用途：持续改 `.missiond/v2/*.lisp`、维护架构上下文、减少重新定位成本。
   - preferred dispatch：复用已有常驻 ClaudeCode slot/session。
   - risk：上下文可能漂移；每次任务仍必须读任务 `.md` 和运行 checker。

2. `fresh-code-alignment-session`
   - 用途：代码同构任务、Rust/SQL/JS 修改。
   - preferred dispatch：新开 ClaudeCode 工位或新 session，因为任务文件自包含且需要隔离。
   - risk：必须靠 `.md` 任务写清楚 scope / no-goals / acceptance。

3. `agent-team-hint`
   - 触发：任务可拆成多个独立读/查/验证子任务，或要跨多个 pillar 定位。
   - 具体约定：在给 ClaudeCode 的任务文字中加入“使用 agent-team 提高效率”。
   - guardrail：并行子 agent 可以读和建议，但最终写入要由主 agent 统一落笔；涉及同一文件同一块时不要并发写。

4. `spawn-over-prompt-mode`
   - 规则：优先通过 MissionD spawn/resident workstation 调度 ClaudeCode，尽量少用 `claude -p` 一次性 prompt。
   - 原因：spawn 工位有 cwd/project-root、MCP config、session logs、execution evidence、可监控状态；`-p` 更难纳入 MissionD 的 evidence / workflow / capability usage 闭环。

5. `project-root-cwd-contract`
   - cross-ref 已有 project-root spawn cwd 设计。
   - 说明：任何新/fresh code alignment 工位必须在目标项目 root spawn。

### 2. `.missiond/v2/intent-flow.lisp`

升级 `F-intent-alignment-plan-execution-loop` 的相关 stages：

- `s2 intent-alignment-authoring`
  - 补 mode B：resident Lisp ClaudeCode slot。
  - 说明 Lisp 架构改动优先复用常驻 Lisp 会话。

- `s4 plan-authoring`
  - 说明规划可用 direct LLM 或 resident planning slot。

- `s6 execution-runner`
  - 增加 `dispatch-strategy`：
    - Lisp-only architecture task → resident-lisp-architect-session。
    - code-alignment implementation task → fresh-code-alignment-session。
    - broad independent scan/refactor → include agent-team hint。
    - project-bound coding → spawn in target project root。
  - 明确 `claude -p` 是 fallback / non-preferred。

可新增一个小 named flow 或 subpath：

- `F-workstation-dispatch-policy`

但不要重复设计已有 full slot lifecycle；只作为 unified-entry pipeline 的策略说明和 cross-ref。

### 3. `.missiond/v2/intent-intent-layer.lisp`

在 `section unified-entry-pipeline` 中增加 role 或 policy：

- `workstation-dispatch-policy`
- 或把它挂到 `plan-runner` / `alignment-author`。

必须表达：

- `alignment-author` 可以是 direct LLM，也可以是 resident ClaudeCode slot。
- `plan-runner` 负责根据 PLAN 类型选择常驻 Lisp 会话、新代码同构会话、agent-team、或普通 tool/flow。
- 任务文件 `.missiond/claudecode/*.md` 是给 fresh sessions 的 self-contained contract。
- 常驻 Lisp 会话的上下文是 asset，但不能替代 checker 和 explicit task file。

### 4. `.missiond/v2/intent-tools.lisp`

不新增 tool。

只在相关 implemented surfaces 添加 cross-ref：

- `mission_task_delegate` / `mission_pty_spawn` / `mission_compute_slot` 如已有条目，补“preferred spawn/resident workstation substrate”说明。
- `mission_execution` 补：execution coordination 可记录 which strategy was used: resident-lisp / fresh-code-alignment / agent-team / prompt-fallback。
- `mission_plan` 补：future plan-runner 会读取 plan 的 dispatch strategy。

### 5. `.missiond/v2/intent.lisp`

同步摘要：

- worker canonical status 提到 claudecode workstation orchestration policy。
- flow canonical status 提到 unified pipeline now includes workstation dispatch strategy。
- intent-layer canonical status 提到 plan-runner dispatch strategy。

## 状态标注要求

请保守标注：

- `architecture-designed`：这次只是 Lisp 设计。
- `code-alignment pending`：如果 Rust/handler 尚未实现自动选择策略。
- `operational-practice`：已经在人工流程中使用，但 MissionD 尚未自动化。

不要把这些策略写成已经自动实现。

## 验收命令

必须运行：

```bash
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check .missiond/v2/intent-worker.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent.lisp
```

不要运行 cargo，因为本任务不改 Rust。

## 交付报告

完成后请报告：

- 修改了哪些 Lisp 文件。
- 常驻 Lisp 会话、新代码同构会话、agent-team、spawn-over-`-p` 分别落到了哪里。
- 哪些是 operational-practice，哪些是 architecture-designed/code-alignment pending。
- 是否新增 tool：应为 no。
- 验收命令结果。

不要 stage，不要 commit。
