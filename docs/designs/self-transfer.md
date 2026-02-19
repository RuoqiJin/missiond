# 自传助手 (Self-Transfer) 设计文档

> 类微信文件传输助手：自己跟自己发消息/图片/文件，扫码登录快速跨设备传内容

## 调研结论

| 基础设施 | 现状 | 复用度 |
|----------|------|--------|
| QR 扫码登录 | xjp-auth 已完整实现 (WeChat QR + SSE 状态推送) | 100% |
| 媒体上传 | presigned URL → R2 (前端 useChatMedia 已有) | 90% |
| JWT 认证 | AuthProvider + 自动刷新 | 100% |
| SSE 实时推送 | assistant 服务已用，auth QR 事件也用 SSE | 模式复用 |
| WebSocket | 仅 deploy-agent 隧道用，主后端无 WS | 不复用 |
| 数据库 | MySQL (GCP) + Redis (session/state) | 复用 |
| 对象存储 | R2 (默认) / XjpFS / OSS，bucket: xiaojinpro | 复用 |

## 架构决策

### 为什么不复用现有 Chat？

| 维度 | AI Chat | 自传助手 |
|------|---------|----------|
| 消息方向 | user ↔ AI | self ↔ self |
| 依赖 | Assistant Session + Router + 计费 | 无 |
| 复杂度 | SSE streaming + tool calling | 简单 CRUD + 实时推送 |
| 数据模型 | conversation → messages + content_parts | 扁平消息列表 |

结论：**独立实现**，但复用 UI 组件库 (Radix) 和上传基础设施 (presigned URL)。

### 实时通信：SSE vs WebSocket vs 轮询

| 方案 | 优点 | 缺点 |
|------|------|------|
| **SSE** | 后端已有模式；HTTP/2 复用；简单 | 单向（server→client）|
| WebSocket | 双向 | 后端无现有支持；需新增基础设施 |
| 短轮询 | 最简单 | 延迟高、浪费带宽 |

**选择 SSE**：
- 发送消息走 POST API（client→server 不需要 WS）
- 接收消息走 SSE（server→client 推送）
- 与现有 QR 登录、Chat streaming 一致
- 跨设备场景下 SSE 比 WS 更稳定（代理/防火墙友好）

---

## 数据模型

### MySQL: `transfer_messages` 表

```sql
CREATE TABLE transfer_messages (
    id          BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    user_id     VARCHAR(36) NOT NULL,           -- 用户 UUID
    msg_type    ENUM('text', 'image', 'file') NOT NULL DEFAULT 'text',
    content     TEXT,                            -- 文本内容 / 图片 caption
    file_url    VARCHAR(1024),                   -- R2 对象 URL
    file_name   VARCHAR(255),                    -- 原始文件名
    file_size   BIGINT UNSIGNED,                 -- 文件大小 (bytes)
    mime_type   VARCHAR(128),                    -- MIME type
    device_tag  VARCHAR(64),                     -- 发送设备标识 (UA hash)
    created_at  DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),

    INDEX idx_user_time (user_id, created_at DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

### Redis: 实时推送通道

```
transfer:sse:{user_id}  →  List<JSON>   // 待推送消息队列，TTL 60s
transfer:online:{user_id}  →  SET<device_id>  // 在线设备集合
```

---

## API 设计

### 归属服务

新增 `transfer` 模块到现有 monolith (端口 3000)，Caddy 路由 `/v1/transfer/*` → monolith。

### 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/v1/transfer/messages?before={id}&limit=50` | 分页加载历史消息 |
| POST | `/v1/transfer/messages` | 发送消息 |
| DELETE | `/v1/transfer/messages/{id}` | 删除单条消息 |
| POST | `/v1/transfer/messages/clear` | 清空所有消息 |
| POST | `/v1/transfer/media/presign` | 获取上传预签名 URL |
| GET | `/v1/transfer/events` | SSE 实时消息流 |

### 发送消息 Request

```json
POST /v1/transfer/messages
Authorization: Bearer {jwt}

// 纯文本
{ "type": "text", "content": "https://example.com/some-link" }

// 图片（先 presign 上传完成后）
{ "type": "image", "file_url": "https://r2.xiaojinpro.top/...", "file_name": "screenshot.png", "file_size": 102400, "mime_type": "image/png", "content": "可选 caption" }

// 文件
{ "type": "file", "file_url": "https://r2.xiaojinpro.top/...", "file_name": "document.pdf", "file_size": 5242880, "mime_type": "application/pdf" }
```

### SSE 事件流

```
GET /v1/transfer/events
Authorization: Bearer {jwt}

event: message
data: {"id":123,"type":"text","content":"hello","device_tag":"chrome-mac","created_at":"..."}

event: delete
data: {"id":123}

event: clear
data: {}

:heartbeat
```

**实现方式**：
1. 客户端 POST 发送消息 → 写入 MySQL + RPUSH 到 Redis 队列
2. SSE handler 循环 BLPOP Redis 队列 + 定期 heartbeat
3. 多设备同时在线 → 每个 SSE 连接独立订阅（Redis Pub/Sub 替代 List）

**修正：用 Redis Pub/Sub**
- 发消息时 `PUBLISH transfer:{user_id} {msg_json}`
- 每个 SSE 连接 `SUBSCRIBE transfer:{user_id}`
- 天然支持多设备广播，无需 List 管理

---

## 前端设计

### 路由

```
/transfer          → 主页面（需登录）
/transfer/qr       → 展示 QR 码供手机扫（已登录设备生成）
```

### 页面布局

```
┌────────────────────────────────────────┐
│  自传助手          [清空] [QR码]       │
├────────────────────────────────────────┤
│                                        │
│  ┌──────────────┐                      │
│  │ 文本消息      │  ← 自己发的，右对齐  │
│  └──────────────┘                      │
│                                        │
│        ┌──────────────┐                │
│        │ [图片预览]    │  ← 另一设备发的 │
│        │ screenshot.png│     左对齐     │
│        └──────────────┘                │
│                                        │
│  ┌──────────────┐                      │
│  │ 📋 一键复制   │  ← 文本消息悬停操作  │
│  │ https://...  │                      │
│  └──────────────┘                      │
│                                        │
├────────────────────────────────────────┤
│ [📎] [输入消息...              ] [发送] │
│       支持粘贴图片 / 拖拽文件           │
└────────────────────────────────────────┘
```

### 核心交互

| 功能 | 实现 |
|------|------|
| **一键复制** | 文本消息 hover/tap 显示复制按钮，`navigator.clipboard.writeText()` |
| **粘贴图片** | 监听 `paste` 事件，读取 `clipboardData.files` → 自动上传发送 |
| **拖拽文件** | `onDrop` → 读取文件 → presign → upload → 发消息 |
| **链接检测** | 文本中 URL 自动高亮可点击 |
| **图片预览** | 点击图片 → 全屏 lightbox |
| **设备标识** | 消息气泡角标显示来源设备 (Chrome/Safari/iPhone 等) |
| **左右分列** | 当前设备发的右对齐，其他设备发的左对齐（用 device_tag 区分）|

### 组件结构

```
src/app/transfer/
  page.tsx              # 主页面
  layout.tsx            # 布局 (窄屏居中，max-w-2xl)
  components/
    TransferInput.tsx   # 底部输入栏 (文本 + 附件 + 粘贴)
    MessageList.tsx     # 消息列表 (虚拟滚动)
    MessageBubble.tsx   # 消息气泡 (文本/图片/文件)
    QRLoginDialog.tsx   # 扫码登录弹窗
    DeviceBadge.tsx     # 设备标识角标
  hooks/
    useTransfer.ts      # 消息 CRUD + SSE 订阅
    useTransferMedia.ts # 媒体上传 (复用 presign 逻辑)
  api.ts                # API 封装
```

---

## QR 扫码登录流程

### 现有基础设施 (xjp-auth，100% 复用)

```
已登录设备(A)                     新设备(B)

打开 /transfer                   打开 /transfer (未登录)
   │                                │
   │                            显示 QR 码页面
   │                            POST /v1/qr-login/start
   │                            GET  /v1/qr-login/events (SSE)
   │                                │
   │  ←── 扫码 ────                │
   │  (手机扫 QR)                   │ (等待中...)
   │                                │
   │  确认授权                      │
   │  ──────────→                  │
   │                            收到 SSE: APPROVED
   │                            GET /oauth2/authorize
   │                            POST /oauth2/token
   │                            ✅ 获得 JWT → 进入 /transfer
```

### 简化 QR 方案（可选，未来优化）

对于纯粹的自传助手场景，可以实现更轻量的 QR 登录：
1. 已登录设备生成临时 token（短 TTL）
2. QR 码编码 `https://xiaojinpro.top/transfer?token={temp_token}`
3. 新设备扫码直接打开页面，用 temp_token 换 JWT

但 MVP 阶段直接用现有微信 QR 登录即可。

---

## 实现计划

### Phase 1: 后端 API (2-3 天)

1. **新建 `transfer` 模块** (services/monolith 或新 service)
   - `transfer_messages` 表 migration
   - CRUD endpoints (messages, presign)
   - Redis Pub/Sub → SSE 事件流

2. **Caddy 路由**
   - `/v1/transfer/*` → monolith:3000

### Phase 2: 前端页面 (2-3 天)

3. **`/transfer` 路由 + 页面**
   - MessageList + MessageBubble (Radix UI)
   - TransferInput (文本 + 粘贴图片 + 拖拽文件)
   - SSE 实时接收 + 乐观 UI 更新

4. **媒体上传**
   - 复用 presigned URL 上传模式
   - 支持粘贴、拖拽、选择文件

### Phase 3: QR 登录集成 (1 天)

5. **QR 登录流程**
   - 未登录时显示 QR 码 (复用 `/v1/qr-login/*`)
   - SSE 监听扫码状态
   - 成功后自动进入 /transfer

### Phase 4: 体验优化 (1-2 天)

6. **优化**
   - 虚拟滚动 (消息量大时)
   - PWA 支持 (手机添加到桌面)
   - 快捷键 (Ctrl+V 粘贴图片, Enter 发送)
   - 消息搜索

---

## 技术风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| SSE 连接断开 | 丢失实时消息 | 重连时从 lastEventId 拉取 gap 消息 |
| 大文件上传慢 | 用户体验差 | 进度条 + presigned 直传 R2 (不经后端) |
| Redis Pub/Sub 消息丢失 | 离线设备收不到 | DB 为主，SSE 为辅；重连时查 DB 补齐 |
| 微信 QR 需手机微信 | 没微信的设备无法扫码 | Phase 2 可加邮箱 OTP 或浏览器推送确认 |

---

## 未来扩展

- **端到端加密**：敏感内容可选 E2E 加密
- **消息过期**：自动清理 N 天前的消息
- **iOS/Android 客户端**：原生推送通知
- **多人传输**：扩展为设备间分享（非仅 self-to-self）
- **剪贴板同步**：后台自动同步剪贴板（需客户端支持）
