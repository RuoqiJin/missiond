# Wave 14 / Task 06 — event domain stale "12 domains" doc cleanup

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

目标：清理代码注释中仍写死 “12 domains / frozen 12-domain contract” 的 stale 文本。当前 `Domain::ALL` 已不是 12，测试不失败，但注释会误导后续 agent。

Ownership：
- `crates/missiond-core/src/event/**/*.rs`
- `crates/missiond-daemon/src/bus/bootstrap.rs`
- `crates/missiond-daemon/src/main.rs`
- `crates/missiond-daemon/src/state.rs`
- `.missiond/v2/intent-event-bus.lisp` 仅限必要的 status wording 修正，不改 event 架构

禁止：
- 不改运行时代码行为
- 不新增 event variant
- 不改 tests 除非只是注释/名称
- 不要 `git add .`

功能要求：
1. 把 stale “12 domains” 改成：
   - “Domain::ALL”
   - “domain set started at 12 and is extensible”
   - 或 “current domain set”
2. 保留历史语义：12 是起点，不是永久上限。
3. 不碰 `_phase0-inventory.md` 这类历史归档，除非它明确声称 current truth。
4. `rg "12 domains|all 12 domains|12-domain|12 frozen domains"` 应只剩历史归档或明确历史上下文。

验收：
- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交：
- scoped commit，只 stage ownership 文件。
- commit message:
  `docs(event): remove stale fixed domain count wording`

交付报告：
- commit hash
- changed files
- remaining rg hits and why they are acceptable

