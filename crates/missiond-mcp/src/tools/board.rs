use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Board Tasks (Personal Task Board) =====
        ToolDefinition::new(
            "mission_board_list",
            "列出个人任务板上的所有任务。返回树形结构（含子任务）。可按状态筛选。默认隐藏 hidden 任务（续费等手动处理项）。",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "按状态筛选: open, done (不传则返回全部)"
                    },
                    "includeHidden": {
                        "type": "boolean",
                        "description": "是否包含隐藏任务（续费等手动处理项）。默认 false"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_board_create",
            "在个人任务板上创建新任务。支持子任务（通过 parentId）。",
            json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "任务标题"
                    },
                    "description": {
                        "type": "string",
                        "description": "任务描述"
                    },
                    "priority": {
                        "type": "string",
                        "description": "优先级: high, medium, low (默认 medium)"
                    },
                    "category": {
                        "type": "string",
                        "description": "分类: deploy, dev, infra, test, other (默认 other)"
                    },
                    "project": {
                        "type": "string",
                        "description": "关联项目 (如 my-project)"
                    },
                    "server": {
                        "type": "string",
                        "description": "关联服务器 (如 私有云, ECS, GCP)"
                    },
                    "dueDate": {
                        "type": "string",
                        "description": "截止日期 (YYYY-MM-DD)"
                    },
                    "parentId": {
                        "type": "string",
                        "description": "父任务 ID (创建子任务时使用)"
                    },
                    "assignee": {
                        "type": "string",
                        "description": "分配的 PTY 工位 ID (如 slot-coder-1)，用于自动执行"
                    },
                    "autoExecute": {
                        "type": "boolean",
                        "description": "是否在 due_date 到达时自动执行 (需要 assignee)"
                    },
                    "promptTemplate": {
                        "type": "string",
                        "description": "自动执行时的 prompt 模板 (不填则用 title + description)"
                    },
                    "hidden": {
                        "type": "boolean",
                        "description": "隐藏任务（续费等手动处理项），不在默认列表中显示"
                    },
                    "flowTemplate": {
                        "type": "string",
                        "description": "Flow 模板名（如 engineering）。设置后任务进入 Flow Engine 模式"
                    },
                    "dependsOn": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "DAG 依赖：前置任务 ID 列表。所有前置任务完成后才会自动执行"
                    }
                },
                "required": ["title"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_update",
            "更新任务板上的任务。可修改标题、状态、优先级等任意字段。",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "任务 ID"
                    },
                    "title": {
                        "type": "string",
                        "description": "新标题"
                    },
                    "description": {
                        "type": "string",
                        "description": "新描述"
                    },
                    "status": {
                        "type": "string",
                        "description": "新状态: open, done"
                    },
                    "priority": {
                        "type": "string",
                        "description": "新优先级: high, medium, low"
                    },
                    "category": {
                        "type": "string",
                        "description": "新分类: deploy, dev, infra, test, other"
                    },
                    "project": {
                        "type": "string",
                        "description": "新关联项目"
                    },
                    "server": {
                        "type": "string",
                        "description": "新关联服务器"
                    },
                    "dueDate": {
                        "type": "string",
                        "description": "新截止日期"
                    },
                    "parentId": {
                        "type": "string",
                        "description": "新父任务 ID"
                    },
                    "assignee": {
                        "type": "string",
                        "description": "分配的 PTY 工位 ID"
                    },
                    "autoExecute": {
                        "type": "boolean",
                        "description": "是否自动执行"
                    },
                    "promptTemplate": {
                        "type": "string",
                        "description": "自动执行 prompt 模板"
                    },
                    "hidden": {
                        "type": "boolean",
                        "description": "隐藏任务（续费等手动处理项）"
                    },
                    "flowPhase": {
                        "type": "string",
                        "description": "Flow 当前阶段: investigate, consult_gemini_1, plan, consult_gemini_2, execute, finalize, done"
                    },
                    "flowTemplate": {
                        "type": "string",
                        "description": "Flow 模板名（如 engineering）"
                    },
                    "dependsOn": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "DAG 依赖：前置任务 ID 列表"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_get",
            "获取任务板上单个任务的详细信息",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_delete",
            "删除任务板上的任务（级联删除所有子任务）",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_toggle",
            "切换任务状态 (open ↔ done)",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_claim",
            "认领任务。原子操作：仅当任务状态为 open 且未被其他执行者认领时成功。防止多个 Claude Code 实例重复执行同一任务。认领成功后任务状态自动变为 running。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "要认领的任务 ID"
                    },
                    "executorId": {
                        "type": "string",
                        "description": "执行者标识 (如 slot ID 或会话标识)。不填则用 MCP session ID"
                    },
                    "executorType": {
                        "type": "string",
                        "description": "执行者类型: pty_slot (自动化工位) 或 manual_session (人工交互)。默认 manual_session"
                    }
                },
                "required": ["taskId"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_note_add",
            "为任务添加进度笔记。用于记录阶段进度、完成摘要或一般备注。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "任务 ID"
                    },
                    "content": {
                        "type": "string",
                        "description": "笔记内容（支持 markdown）"
                    },
                    "noteType": {
                        "type": "string",
                        "description": "笔记类型: progress (进度更新), summary (完成摘要), note (一般备注)。默认 note"
                    },
                    "author": {
                        "type": "string",
                        "description": "作者标识 (如 claude-code, user)"
                    }
                },
                "required": ["taskId", "content"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_summary",
            "任务板执行摘要。返回各状态任务计数、待处理问题数、新 KB 条目数。用于用户回来后快速了解全貌。",
            json!({
                "type": "object",
                "properties": {
                    "since": {
                        "type": "string",
                        "description": "起始时间(ISO 8601)，统计该时间之后完成/失败的任务和新增 KB。不传则统计全部"
                    }
                }
            }),
        ),
        // ===== Task Decompose =====
        ToolDefinition::new(
            "mission_board_decompose",
            "一键拆分任务。派 Opus 工位调查代码后，自动创建带 DAG 依赖链的子任务序列，关键节点插入 user_review 检查点。\
             工位会 Read/Grep 调查相关代码、查 KB/Skill，然后调 mission_board_create 创建子任务。\
             返回 submit task ID（异步执行）。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "要拆分的父任务 ID"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "执行拆分的工位 ID (默认 slot-coder-1)"
                    },
                    "hints": {
                        "type": "string",
                        "description": "用户对拆分方向的提示（如: 重点关注性能、先调查数据库schema）"
                    }
                },
                "required": ["taskId"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_retry",
            "重试失败/阻塞的任务。重置任务状态为 open 并解除 claim，可选级联重置所有下游任务。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "要重试的任务 ID"
                    },
                    "resetDownstream": {
                        "type": "boolean",
                        "description": "是否同时重置所有下游依赖任务 (默认 true)"
                    }
                },
                "required": ["taskId"]
            }),
        ),
        // ===== Engineering Flow =====
        ToolDefinition::new(
            "mission_submit_phase_result",
            "工程任务流程中，完成当前阶段后调用此工具提交产出物。系统自动推进到下一阶段。\
             仅在 flow 任务（flow_phase 非空）的 Slot 阶段使用。\
             如果对结果没信心，可用 requiresMasterDecision 触发主控审核。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "Board 任务 ID"
                    },
                    "artifactType": {
                        "type": "string",
                        "enum": ["investigation_report", "execution_plan", "execution_result", "commit_hash"],
                        "description": "产出物类型，必须与当前阶段匹配"
                    },
                    "content": {
                        "type": "string",
                        "description": "产出物内容"
                    },
                    "requiresMasterDecision": {
                        "type": "string",
                        "description": "如果对当前产出物拿不准，在此填写困惑描述，触发 Decision Engine 软拦截审核"
                    }
                },
                "required": ["taskId", "artifactType", "content"]
            }),
        ),

    ]
}
