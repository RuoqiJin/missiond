# 基础设施数据安全管理

## 问题

MissionD 准备开源，需确保：
1. 仓库中零敏感数据（IP、密码、API Key）
2. 本地配置文件有适当的访问控制
3. 密钥通过加密通道管理，不明文存储

## 三层架构

```
Layer 1: 代码 (public)          — GitHub 仓库，任何人可见
Layer 2: 本地配置 (~/.xjp-mission/)  — chmod 600，仅本机用户可读
Layer 3: 远程加密 (Secret Store)     — AES-256-GCM，RBAC 权限控制
```

### Layer 1 — 代码层

| 允许 | 禁止 |
|------|------|
| 类型定义、schema | IP 地址、域名 |
| 工具描述（不含示例值） | API Key、密码 |
| 默认配置模板 | 账号名、邮箱 |
| 文档（不含凭据） | 续费日期、费用金额 |

**审计方式**: `git log --all -p` 全量搜索 + CI pre-commit hook

### Layer 2 — 本地配置

位置: `~/.xjp-mission/`

| 文件 | 内容 | 权限 |
|------|------|------|
| `servers.yaml` | 服务器元数据（IP、位置、角色） | 600 |
| `slots.yaml` | 工位配置（角色、MCP 路径） | 600 |
| `xjp-mcp-config.json` | MCP 服务器启动配置 | 600 |
| `config/permissions.yaml` | 权限策略 | 600 |
| `mission.db` | 任务/对话/知识库 | 600 |

**保护措施**:
- daemon 启动时自动检查并修复文件权限为 600
- 不入 git（路径在 `~` 下，与仓库隔离）

### Layer 3 — 远程加密

通过 `xjp_secret_get/set` MCP 工具访问 Secret Store 服务。

| 数据 | 存储位置 | 访问方式 |
|------|---------|---------|
| GitHub PAT | `github-shared/github_pat` | `xjp_secret_get` |
| Deploy Agent API Key | `deploy-shared/deploy_agent_api_key` | `xjp_secret_get` |
| 数据库密码 | `gcp/database_url` | `xjp_secret_get` |
| 服务间 token | 各命名空间 | `xjp_secret_get` |

## 数据流

```
启动:
  servers.yaml ──→ InfraConfig (内存) ──→ mission_infra_list/get
  slots.yaml   ──→ SlotManager (内存) ──→ PTY spawn
  xjp-mcp-config.json ──→ --mcp-config 传给 claude

运行时:
  Agent 调用 mission_infra_get("ecs") → 返回服务器信息 (JSON)
  Agent 调用 xjp_secret_get("key")   → Secret Store → 返回值
  Agent 调用 mission_submit(prompt)   → prompt 中不含基础设施信息
                                         Agent 需主动查询
```

## 当前安全状态

| 项目 | 状态 | 说明 |
|------|------|------|
| 仓库无 hardcoded 密钥 | ✅ | 代码已清理，git 历史待开源前 BFG 清理 |
| 配置文件权限 | ⚠️ → ✅ | Phase 1 自动修复为 600 |
| MCP config 明文 API Key | ⚠️ | Phase 2 环境变量模板替换 |
| mission.db 加密 | ❌ | Phase 3 考虑 SQLCipher |
| 审计日志 | ❌ | Phase 3 MCP 调用审计 |

## 实施路线

### Phase 1 — 最小安全加固 (本次)

1. **启动权限检查**: daemon 启动时自动 chmod 600 关键配置文件
2. **仓库审计**: 代码层已清理（`tools.rs` 示例 IP 替换为 `10.0.0.1`）
3. **本设计文档**: 记录架构和规划
4. **开源前**: 需 BFG 清理 git 历史中 `tunnel-anti-gfw.md` 的 GCP IP、`jarvis-kb.md` 的内网 IP

### Phase 2 — MCP Config 安全化

**问题**: `xjp-mcp-config.json` 中 `DEPLOY_AGENT_API_KEY` 明文存储

**方案**: 环境变量模板

```json
{
  "env": {
    "DEPLOY_AGENT_API_KEY": "$ENV:DEPLOY_AGENT_API_KEY"
  }
}
```

MissionD 读取 MCP config 时，`$ENV:` 前缀从进程环境变量替换，`$SECRET:` 前缀从 Secret Store 获取。

敏感值来源:
- `~/.xjp-mission/secrets.env` (chmod 600) — 简单方案
- Secret Store API — 完整方案（需 Rust 客户端）

### Phase 3 — 深度加固

1. **mission.db 加密**: SQLCipher 或 `PRAGMA key`
2. **MCP 调用审计日志**: 记录谁在什么时候调用了什么工具
3. **Secret Store Rust 客户端**: MissionD 直接访问 Secret Store，不依赖 xjp-mcp
4. **运行时注入**: 任务下发时按需注入基础设施信息，Agent 不需主动查询
