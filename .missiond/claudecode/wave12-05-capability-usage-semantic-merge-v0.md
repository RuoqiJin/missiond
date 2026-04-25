# Wave 12 / Task 05 — capability_usage semantic merge review v0

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：必须在 Wave 11 scoped commit 完成后执行。

目标：在已有 mission_capability_usage semantic evidence v1 上，增加“merge review v0”：只产候选和 review sidecar，不自动改 registry、不 destructive。

Ownership：
- `crates/missiond-daemon/src/handlers/comm/capability_usage.rs`
- `crates/missiond-mcp/src/tools/comm/capability_usage.rs`

不要修改：
- `.missiond/v2/*.lisp`
- knowledge/plan/workflow/agent_execution
- DB migrations

功能要求：
1. 新增/完善 candidates 输出的 merge review 字段：
   - `replacement_target`
   - `replacement_confidence`
   - `semantic_hint_source`
   - `review_required`
   - `protected_target_policy`
2. `mark(decision=merge)` 必须：
   - 要求 replacement_target
   - 拒绝 self target
   - 拒绝不存在 target
   - protected source 或 protected target 时拒绝 destructive mark，只允许 review-only mark
3. sidecar `capability-usage-review.json` 记录 replacement_target 和 decision rationale。
4. 不做 fuzzy semantic merge，不改 tool/flow registry。

测试要求：
- protected source/target tests
- replacement target validation tests
- sidecar backward compatibility test
- source_coverage 仍包含 6 lanes
- `cargo test -p missiond-daemon handlers::comm::capability_usage::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

交付：
- scoped commit，只 stage 本任务 ownership 文件。
- commit message 建议：
  `feat(capability-usage): add semantic merge review state`
- 报告 commit hash、merge review schema、拒绝策略。

