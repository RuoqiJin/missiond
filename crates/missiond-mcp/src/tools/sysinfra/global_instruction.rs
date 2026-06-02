//! mission_global_instruction — daemon-side manager for the global
//! Claude Code instruction file at ~/.claude/CLAUDE.md.
//!
//! Lisp authority:
//!   - intent-intent-layer.lisp :: global-claudemd-manager
//!   - intent-tools.lisp :: future-surface mission_global_instruction
//!   - intent-flow.lisp :: trivial-single-step read/edit/reload
//!
//! Status: code-alignment for read/edit (full) + reload (manual-reload-required
//! honest stub — no daemon-side hot-reload exists for global CLAUDE.md, since
//! Claude Code reads it once per session at bootstrap).

use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_global_instruction",
        "全局 ~/.claude/CLAUDE.md 管理器 — 3 actions (read/edit/reload)。\
         read: 返回内容 + path/size/mtime/sha256; \
         edit: 接收 new_content + dry_run + allow_empty, 写入前生成时间戳备份, \
         通过 临时文件 + atomic rename 落盘, 仅允许操作 ~/.claude/CLAUDE.md; \
         reload: daemon 不持有该文件 (Claude Code 会话启动时一次性读取), \
         返回 manual-reload-required 状态, 不假装 reload 成功。\
         ClaudeCode instruction projections should prefer missiond-mcp/xjp-mcp, \
         using missiond-cli/xjp-cli only for gap-fill or diagnostics. \
         Lisp 源: intent-intent-layer.lisp :: global-claudemd-manager + \
         intent-tools.lisp :: future-surface mission_global_instruction。",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "edit", "reload"],
                    "description": "read=读取全文+元数据; edit=覆写(temp+rename, 备份); reload=查询 daemon 侧是否支持热重载(当前不支持, 返回 manual)"
                },
                "new_content": {
                    "type": "string",
                    "description": "[edit] 新文件内容(UTF-8). 必须显式提供 — 本工具不接受 patch 形式"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "[edit] 为 true 时只回 preview/diff, 不写入也不生成备份 (默认 false)",
                    "default": false
                },
                "allow_empty": {
                    "type": "boolean",
                    "description": "[edit] 允许写入空内容 (默认 false — 拒绝意外清空)",
                    "default": false
                }
            }
        }),
    )]
}
