# Wave 12 / Task 04 — workflow methodology semantic lifting v0

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：必须在 Wave 11 scoped commit 完成后执行。

目标：升级 `mission_workflow(action=compile_methodology, compile_mode=deterministic)`，从只抽 `(step ...)` 提升到能保守提取 methodology Lisp 的高阶语义，仍然不调用 LLM、不做 forge compiler。

Ownership：
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

不要修改：
- plan/directive/agent_execution/capability_usage
- `.missiond/v2/*.lisp`
- DB migrations

功能要求：
1. 在现有 deterministic compiler 基础上，保守识别：
   - `(phase ...)`
   - `(principle ...)`
   - `(anti-pattern ...)`
   - `(gate ...)`
   - `(artifact ...)`
   - `(authority ...)`
2. 这些语义进入 generated YAML metadata，不要强行变成可执行 node，除非明确有 `(step ...)`。
3. 当 phase 内包含 step，生成 node metadata 要带 `phase_id`。
4. 无 step 但有 phase/principle 时，仍生成 `manual_review` 节点，并把 lifted metadata 写进去。
5. `run_methodology` 保持现有行为，不引入新 runner。

测试要求：
- parser tests：phase with nested step、principle extraction、anti-pattern extraction、string paren safe。
- YAML round-trip tests：metadata 可被 loader 忽略但保留在 raw YAML。
- `cargo test -p missiond-daemon handlers::knowledge::workflow::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

交付：
- scoped commit，只 stage 本任务 ownership 文件。
- commit message 建议：
  `feat(workflow): lift methodology semantics into generated flows`
- 报告 commit hash、提取语义清单、保守边界。

