# MissionD wave 11 code-alignment: review gate QuestionEvent

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

建议等 directive/plan file-first writer 完成后执行，避免同时改 `directive.rs` / `plan.rs`。

## 目标

把 alignment / plan review gate 从纯人工操作推进到 event-bus aware：

- directive compile/persist 后，可选发 `QuestionEvent::Created` 提醒 review。
- plan compile/persist 后，可选发 `QuestionEvent::Created` 提醒 review。
- approve/archive/mark/supersede 时，可选发 resolved/decision event。

不要实现完整人机 UI，也不要阻塞等待回答。本批只做事件化 review gate 的最小可观测闭环。

## Lisp 锚点

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s3 alignment-review-gate + s5 plan-review-gate`
- `.missiond/v2/intent-intent-layer.lisp :: unified-entry-pipeline :: role alignment-review-gate / plan-review-gate`
- `.missiond/v2/intent-event-bus.lisp :: QuestionEvent`

## 写入范围

允许修改：

- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

如必须扩展 event payload，允许修改：

- `crates/missiond-core/src/event/events/question.rs`
- `crates/missiond-core/src/event/events/mod.rs` only if needed
- `crates/missiond-daemon/src/bus/bootstrap.rs` only if helper needed

不要修改：

- workflow.rs
- DB migrations
- `.missiond/v2/*.lisp`

## 行为要求

新增 optional fields：

- `emit_review_question: bool` default false
- `review_question_text: string` optional
- `review_question_id: string` optional override, otherwise deterministic id from artifact id/version/action

For directive compile persist:

- If `emit_review_question=true`, publish QuestionEvent Created after draft is persisted.
- Response includes `review_question_emitted`, `review_question_id`.
- Bus publish failures are logged/surfaced as warning field, not fatal to persisted draft.

For plan compile persist:

- Same behavior, tied to plan_id/version/board_task_id.

For approve/archive/mark/supersede:

- If `review_question_id` provided, publish `QuestionEvent::Resolved` or `DecisionResolved` best-effort.

If current QuestionEvent payload is too small, extend additively only. Existing serde tests must still pass.

## Tests

At minimum:

- deterministic question id helper.
- compile response includes emission fields in dry-run/testable helper.
- QuestionEvent serde round-trip if extended.
- Bus failure does not fail core action, if testable.

## 验收

- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon handlers::knowledge::directive::tests`
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

## 交付报告

说明：

- 哪些 action 会 emit。
- event payload 是否扩展。
- bus failure 语义。
- 未实现的 UI/wait-for-answer 边界。

