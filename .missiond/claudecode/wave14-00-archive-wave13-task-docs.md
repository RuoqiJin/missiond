# Wave 14 / Task 00 — archive Wave13 task briefs

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

目标：把 Wave13 的任务书文件纳入版本库。不要做功能改动。

当前预期状态：
- 代码工作树干净。
- 只有 `.missiond/claudecode/wave13-*.md` 和可能的 `.missiond/claudecode/wave14-*.md` untracked。

允许 stage：
- `.missiond/claudecode/wave13-*.md`

禁止：
- 不要 stage `.missiond/claudecode/wave14-*.md`
- 不要修改 Rust / Lisp / SQL / JS
- 不要 `git add .`

验收：
- `git status --short`
- `git diff --check -- .missiond/claudecode/wave13-*.md`
- 若代码工作树不干净，停止并报告，不要 stage。

提交步骤：
1. `git add .missiond/claudecode/wave13-*.md`
2. `git diff --cached --name-only` 必须只包含 wave13 md。
3. commit message:
   `chore(wave13): archive task briefs`

交付报告：
- commit hash
- staged 文件列表
- 是否仍有 wave14 untracked 文件

