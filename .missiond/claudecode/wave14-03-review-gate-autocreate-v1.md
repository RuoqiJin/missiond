# Wave 14 / Task 03 — review gate auto-create v1 for file-first artifacts

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：建议在 Task 01 后执行，因为 file-first writer response 会提供 file path / artifact metadata。

目标：把 review gate 从“调用方显式 emit_review_question”升级为“artifact 写入后可按策略自动创建 QuestionEvent”。不实现 UI，不等待回答，不自动 approve。

Ownership：
- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs` 仅 workflow artifact review 需要时
- `crates/missiond-mcp/src/tools/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

禁止：
- 不要修改 `.missiond/v2/*.lisp`
- 不要修改 event core schema（除非绝对必要，先报告）
- 不要新增 DB migration
- 不要 `git add .`

功能要求：
1. 新增 review gate policy 参数：
   - `review_gate`: `manual|emit_question|off`
   - default：保持现有行为，不突然多发 event；建议 `manual`
2. 当 `review_gate=emit_question` 且 artifact persist/write 成功：
   - directive compile 创建 alignment review question
   - plan compile 创建 plan review question
   - workflow distill/compile_methodology 可创建 workflow review question（若 scope 已清晰）
3. deterministic question id 包含 artifact kind、id/version、topic 或 file path hash。
4. response 明确：
   - `review_question_emitted`
   - `review_question_id`
   - `review_gate_policy`
   - `review_question_warning`（bus failure）
5. approve/mark/supersede/archive 继续支持 `review_question_id` resolution path。
6. 不等待人回答；不自动 approve。

测试要求：
- review_gate pure id derivation tests。
- directive/plan compile review_gate=emit_question response tests。
- bus failure helper tests if mockable; otherwise pure builder tests。
- legacy no-param behavior unchanged。
- `cargo test -p missiond-daemon handlers::knowledge::review_gate::tests`
- `cargo test -p missiond-daemon handlers::knowledge::directive::tests`
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交：
- scoped commit，只 stage ownership 文件。
- commit message:
  `feat(review): auto-create review questions for artifacts`

交付报告：
- commit hash
- review_gate policy matrix
- event id derivation
- explicit non-goals

