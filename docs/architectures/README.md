# Architecture Manifests

AI Agent 寻路地图。每个 YAML 文件描述一个项目的架构骨架。

## 使用方式

**AI Agent 排查问题时**：先读对应项目的 YAML → 定位模块 → 读 trait/interface → 读实现

**更新时机**：crate/module 新增删除、trait 签名变更、数据流变更、部署拓扑变更

## 项目索引

| 项目 | 文件 | 技术栈 | 描述 |
|------|------|--------|------|
| MissionD | [missiond.yaml](missiond.yaml) | Rust + Tokio + SQLite | Claude Code 多实例编排 |
| CutHub | [cuthub.yaml](cuthub.yaml) | Next.js 16 + React 19 | 视频编辑协作平台 |
| XJP Backend | [xjp-backend.yaml](xjp-backend.yaml) | Rust + Axum + PostgreSQL | 微服务后端核心 |

## 系统全景

```mermaid
graph TB
    subgraph "用户入口"
        CutHub[CutHub 前端<br/>Next.js 16]
        Jarvis[Jarvis Web/iOS]
        ClaudeCode[Claude Code CLI]
    end

    subgraph "MissionD 编排层"
        MCP[MCP Server<br/>stdio JSON-RPC]
        Daemon[Daemon<br/>状态管理 + handler]
        PTY[PTY Manager<br/>多 Claude 实例]
        KB[Knowledge Base<br/>SQLite + 向量]
        EventBus[EventBus<br/>事件总线]
    end

    subgraph "XJP Backend 服务层"
        Auth[Auth Service<br/>OIDC/JWT/API Key]
        Router[AI Router<br/>模型路由 + 计费]
        Timeline[Timeline<br/>项目版本]
        ASR[ASR Service<br/>语音转文字]
        Deploy[Deploy Center<br/>CI/CD]
        Storage[Object Storage<br/>R2/OSS]
        Payments[Payments<br/>Stripe]
    end

    subgraph "外部依赖"
        Claude[Claude API]
        Gemini[Gemini API]
        MiniMax[MiniMax API]
        R2[Cloudflare R2]
        Supabase[Supabase]
        Stripe[Stripe]
    end

    ClaudeCode -->|MCP stdio| MCP
    MCP -->|IPC socket| Daemon
    Daemon --> PTY
    Daemon --> KB
    Daemon --> EventBus
    PTY -->|spawn| Claude

    Jarvis -->|WebSocket| Daemon

    CutHub -->|API| Auth
    CutHub -->|API| Timeline
    CutHub -->|API| ASR
    CutHub -->|CDN| Storage

    Router --> Claude
    Router --> Gemini
    Router --> MiniMax
    Daemon -->|Router Chat| Router

    Timeline --> R2
    Storage --> R2
    CutHub --> Supabase
    Payments --> Stripe
```

## 数据流速查

### MissionD: MCP 工具调用
```mermaid
sequenceDiagram
    participant CC as Claude Code
    participant MCP as MCP Server
    participant D as Daemon
    participant H as Handler
    participant DB as SQLite

    CC->>MCP: JSON-RPC tool call
    MCP->>D: IPC request
    D->>H: dispatch by tool name
    H->>DB: query/mutate
    DB-->>H: result
    H-->>D: response
    D-->>MCP: IPC response
    MCP-->>CC: JSON-RPC result
```

### XJP Backend: AI 路由
```mermaid
sequenceDiagram
    participant C as Client
    participant MW as Middleware
    participant R as Router Service
    participant P as Provider
    participant A as Auth Service

    C->>MW: POST /v1/chat/completions
    MW->>MW: trace_id + auth(XJPKey→AuthContext)
    MW->>R: authenticated request
    R->>R: select provider + model
    R->>A: check credits
    A-->>R: ok
    R->>P: forward to Claude/Gemini/...
    P-->>R: streaming response
    R-->>C: SSE stream
```

### CutHub: 页面加载
```mermaid
sequenceDiagram
    participant U as User
    participant MW as Middleware
    participant AG as AuthGuard
    participant RQ as React Query
    participant API as Timeline API

    U->>MW: visit /dashboard
    MW->>MW: check xjp_has_session cookie
    MW->>AG: render page
    AG->>AG: useAuth() → check user
    AG->>RQ: useProjects()
    RQ->>API: GET /v1/timeline/projects
    API-->>RQ: project list
    RQ-->>AG: cached data
    AG-->>U: render dashboard
```
