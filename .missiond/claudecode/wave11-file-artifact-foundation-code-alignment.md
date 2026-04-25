# MissionD wave 11 code-alignment: file-first artifact foundation

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 背景

Lisp 已明确：

- `.missiond/alignment/<topic>/intent-alignment.lisp` 是 directive/alignment 的 file-first SSOT。
- `.missiond/plans/<topic>/PLAN.lisp` 是 plan 的 file-first SSOT。
- `.missiond/workflows/<topic>.lisp` 是 workflow 的 file-first SSOT。
- DB 行是可查询镜像 + 状态管理面。

当前代码只写 directive/plan/workflow DB row 和部分 sidecar，尚无统一文件写入 helper。本任务只做 foundation，不接具体 handler。

## Lisp 锚点

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: :file-vs-db-contract`
- `.missiond/v2/intent-memory.lisp :: directive-layer :: file-first-artifacts`
- `.missiond/v2/intent-intent-layer.lisp :: unified-entry-pipeline :: :file-first-ssot`

## 写入范围

允许新增/修改：

- `crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs` (new)
- `crates/missiond-daemon/src/handlers/knowledge/mod.rs` (module declaration only)

不要修改：

- `directive.rs`
- `plan.rs`
- `workflow.rs`
- `crates/missiond-mcp/**`
- `crates/missiond-core/**`
- `.missiond/v2/*.lisp`

## 目标 API

新增一个小 helper module，供后续 directive/plan/workflow writer 调用。

建议 API：

- `ArtifactKind::{IntentAlignment, Plan, Workflow}`
- `ArtifactSpec { kind, topic, project_root, file_name? }`
- `sanitize_topic_segment(input: &str) -> String`
- `artifact_path(project_root: &Path, kind: ArtifactKind, topic: &str) -> PathBuf`
- `atomic_write_artifact(path: &Path, content: &str, overwrite: bool) -> Result<WriteOutcome>`
- `read_existing_metadata(path: &Path) -> Result<Option<ArtifactMetadata>>`

路径约定：

- alignment: `<project_root>/.missiond/alignment/<topic>/intent-alignment.lisp`
- plan: `<project_root>/.missiond/plans/<topic>/PLAN.lisp`
- workflow: `<project_root>/.missiond/workflows/<topic>.lisp`

写入规则：

- 自动创建父目录。
- 默认不覆盖已存在文件；`overwrite=true` 才替换。
- 使用临时文件 + rename 原子写入。
- 返回 `path`, `created`, `overwritten`, `sha256`, `bytes`.
- 不写 DB，不发 event。

## 单测要求

至少覆盖：

- topic sanitize 稳定、空值 fallback。
- 三种 ArtifactKind 路径正确。
- atomic write 创建父目录。
- overwrite=false 时拒绝覆盖。
- overwrite=true 时替换。
- sha256/bytes metadata 正确。

## 验收

- `cargo test -p missiond-daemon handlers::knowledge::file_artifacts`
- `cargo test -p missiond-daemon`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

## 交付报告

列出：

- 修改文件。
- API 名称与路径约定。
- overwrite 语义。
- 单测列表。
- 未触碰文件确认。

