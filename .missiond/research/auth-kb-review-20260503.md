# Auth KB Review Report

- Generated: 2026-05-03T11:35:00Z
- Reviewer: master-control (主控直接接手；swarm 派的 3 个 worker 路由到 gemini-ultra slot 后因工位约束全部 blocked，详见 board task notes)
- Board Task: `0426d502-ce0e-4390-a87a-a669a4974c8e`
- SSOT scope: `services/auth/.missiond/intent.lisp` (主, 410L) + `intent-db-identity.lisp` (55L) + `intent-db-oauth.lisp` (115L) + `intent-db-session.lisp` (125L) + `intent-db-iam.lisp` (187L) + `intent-state.lisp` (207L) — 共 6 文件 ~1099 行，覆盖：port/issuer/JWT 配置、35 张表、4 个状态机、~89 API 端点、auth providers、middleware stack、Cedar IAM、Redis cache、key rotation、audit taxonomy、安全不变量
- Total candidates scanned: **29**（含跨服务消费方）
- 真正属于 auth 服务 (projectId=xjp-auth): **4**

> **核心观察**：auth 服务自身 KB 仅 4 条，大部分 auth 相关 KB 实际是**消费方**（jarvis/pcea/cuthub/router）记录的"如何调用 auth"。这些应留在消费方项目 KB，不应被 auth 服务的 lisp 整合。同时**auth 服务缺乏 bugfix/incident 类 ADR**，建议未来对 auth 的 reuse-detection / Cedar timeout / WeChat callback 异常处理等关键事故沉淀为 KB load-bearing 条目。

---

## 1. Superseded by SSOT lisp (建议归档)

事实已 100% 进 SSOT lisp，可加 `superseded-by-lisp` 标签后归档。

| KB ID | key | category | reason | mapped lisp section |
|---|---|---|---|---|
| `4fa5ed15-771f-4ca7-b5dd-0d38d5b32730` | `xjp-auth-wechat-service-oauth-flow` | memory:architecture (xjp-auth) | "微信内一键登录用服务号网页授权 OAuth 流程，非 PC 端 qrconnect 扫码" — 已显式拆分为 `/auth/wechat/qr` (Open Platform) 与 `/auth/wechat/authorize` (smart routing QR vs MP) | intent.lisp `pillar api`「Auth: WeChat」+ `pillar auth-providers (component wechat :doc "QR (Open Platform) + MP (Service Account)")` |
| `6a4a7a48-8b32-41a1-a03f-32fee6af6c52` *(部分)* | `xjp-auth-center-architecture-and-flow` | memory:architecture (xjp-auth) | "端口 8081 via Caddy" + "QR 登录流：POST start → SSE poll → WeChat callback → OAuth code" 全部入 lisp | intent.lisp `(port 8081 :env "PORT")` + intent-state.lisp `qr-login-lifecycle` (none→pending→scanned→authorized→approved→completed→success, 含 SSE poll 800ms/heartbeat 15s) |

**注**：`6a4a7a48` 中关于「前端 JWT 存 localStorage / CutHub Refresh Token HttpOnly Cookie + BFF /api/auth/refresh」是消费方约束，不在 auth 服务 lisp 范围，归入 §4 load-bearing。建议**拆分该条**：核心 architecture 部分归档，前端 token 存储约定保留并迁移到 cuthub/xiaojinpro-frontend 项目 KB。

**superseded count: 2** (其中 1 条建议先拆分)

---

## 2. Promote to lisp constants (建议提炼为常量)

高频路径/端口/服务关系/子域名分配等常量，建议提到 `services/auth/.missiond/intent.lisp` 顶部新增 `(constants ...)` 块或扩充 `(downstream-services ...)`。

| KB ID | key | constant kind | proposed lisp block | reason |
|---|---|---|---|---|
| `1c69a047-5ffe-4c15-af18-313e9d7ebd7c` | `auth-deploy-center-subdomain-allocation` | subdomain | `(constants (subdomain auth "auth.xiaojinpro.com") (subdomain deploy "deploy.xiaojinpro.com") (dns-provider "cloudflare"))` | "auth.xiaojinpro.com → 认证服务 / deploy.xiaojinpro.com → 部署中心". 当前 lisp 只在 ISSUER `:example` 字段隐含一次，且**写的是 `auth.xiaojinpro.top` 而非 `.com`** — ⚠️ **冲突需用户确认**。多处 KB（jarvis-https-channel-architecture / ios-jarvis / cuthub）均使用 `.com`，疑似 `.top` 是规划中或测试域。此条 KB 高频访问 (362 次) 表明确为 hot constant |

**promote count: 1** ⚠️ 标记 **DOMAIN-CONFLICT**：`.top` (lisp) vs `.com` (KB+多处消费方记录)

---

## 3. Outdated or wrong (建议删除，需用户确认)

| KB ID | key | conflict with lisp | last verified | reason |
|---|---|---|---|---|
| `f6589b56-95ff-49ca-8870-eda431755294` | `auth-mysql-to-postgres-migration-plan` | lisp 已声明 `(database "postgresql")`；intent-db-*.lisp 的 35 张表全部使用 `bigserial`/`timestamptz`/`bytea`/`jsonb` 等 PG 原生类型 | 2026-03-12 (KB createdAt) | "Auth MySQL → PostgreSQL 迁移确认：134 Rust 文件、21 个 MySqlPool" 是 Phase 3B migration plan，迁移已完成，操作价值已过期。仅作历史 ADR 可保留但需加 `historical` 标签 |

**delete count: 1** （建议确认后删除，或保留为 historical）

---

## 4. Load-bearing — keep (保留)

不在 lisp 中、且仍有价值的 auth 服务关联约束。

| KB ID | key | category | rationale why not in lisp |
|---|---|---|---|
| `6a4a7a48-8b32-41a1-a03f-32fee6af6c52` *(残余部分)* | `xjp-auth-center-architecture-and-flow` (split: token-storage portion only) | memory:architecture | 消费方约定：「默认前端存 JWT 于 localStorage 并自动刷新；CutHub 已升级为混合存储（Access Token localStorage + Refresh Token HttpOnly Cookie + BFF /api/auth/refresh）」— 这是 cuthub/xiaojinpro-frontend 的存储策略，不属 auth 服务 SSOT。建议**拆分本条**：删除已 superseded 的架构部分，保留 token storage convention，可考虑迁移到 `cuthub` 或 `xiaojinpro-frontend` 项目 KB |

**keep count: 1** （拆分后保留的部分）

> **缺口**：未发现任何 auth 服务自身的 `memory:bugfix` / `memory:debug` / `memory:incident` 条目。建议主控未来主动在 auth-related session 中触发提取，沉淀以下高价值事件：
> - refresh token reuse-detection 误判事故（30s grace window 设计 rationale）
> - JWT key rotation grace period 失败案例
> - WeChat callback HMAC state 验证失败排查
> - Cedar PDP timeout fail-closed 实操经验
> - auth_codes 表 used_at 并发竞争修复
>
> 这些都是 lisp 不应承载、但跨会话有价值的 ADR。

---

## 5. Out-of-scope (跳过，仅外围 — 由各自 SSOT 维护)

这些条目记录的是消费方 / 兄弟服务如何与 auth 交互。它们本身是**消费方 KB**，与 auth 服务的 lisp 整理无关。建议本次评估完全跳过，由各自项目（jarvis/pcea/cuthub/router/deploy-center/missiond/xiaojinpro-backend）独立审视。

| KB ID | key | owner project |
|---|---|---|
| `ed8cc1d7-c4c9-472a-9ca9-a4e5001064fb` | jarvis-https-channel-architecture | jarvis |
| `b4a82c5e-d4d9-48ef-a44f-1b8f04e13e9f` | ios-jarvis-integration-architecture | jarvis |
| `12b794f5-6e65-4ca0-93be-9ceb90e31ebe` | jarvis-phase2-e2e-completed | jarvis |
| `6e9e171f-4509-4e2b-a891-9be631fd0788` | frontend-chat-dual-session | jarvis |
| `f3dbf26d-bf7b-44e3-a9bf-6dde33910930` | pcea-api-auth-architecture | pcea-video-vault |
| `1ab1ab0e-7768-4e1d-b213-51f4f12b508b` | pcea-admin-users-tenant-isolation | pcea-video-vault (memory:bugfix) |
| `0b1159c6-75c7-486f-bdf2-11b8c05ef3fa` | multi-tenant-admin-api-four-layer-defense | xiaojinpro-backend |
| `cd02eeb4-a8be-4cb2-86b5-f7bb0d628d37` | strategic-pref-admin-权限防线 | pcea-video-vault (architecture:security) |
| `8e6fd981-4fa7-458d-b71a-d3261c818285` | pcea-admin-preview-mode-effectiveUser-hijack | pcea-video-vault |
| `94c2fffe-ed89-40b5-b178-9f79e410fedf` | pcea-non-admin-route-protection | pcea-video-vault |
| `3d9bee88-900b-4f0f-8f12-df12845e987d` | pcea-frontend-backend-separation-2026-03-15 | pcea-video-vault |
| `e00cb083-4c48-4e8e-9660-5ade0bf3bd68` | cuthub-auth-optimistic-rendering-from-jwt | cuthub |
| `04ef779a-c9ff-4e11-a273-4b8081206244` | xjp-router-cpapi-claude-model-endpoints | xjp-router |
| `75df6f78-b752-4e80-bfef-85cc99736832` | router-billing-three-layer-system | missiond (router 内部架构) |
| `a3c827ef-8406-4647-a312-0a475d293cbe` | frontend-never-direct-auth-calls | xiaojinpro-backend |
| `12c114dd-cb11-4723-8600-df9986ccc99a` | xjp-monolith-extracted-services-status | xiaojinpro-backend |
| `311ec549-92d2-45e8-af0e-ea81577dbe25` | xjp-backend-monolith-split-plan | xiaojinpro-backend |
| `725536bd-b23b-44f2-ab67-e4bcca0ca9d7` | backend-bff-deprecated-migration-complete | xiaojinpro-backend (memory:ops) |
| `b9a109ef-8ac2-4f1a-9593-3a6b891d1eb4` | backend-unified-key-management-architecture | xiaojinpro-backend |
| `57f684e3-e09d-49d3-a011-d0d6b01ecaae` | deploy-center-github-oidc-dual-auth | xjp-deploy-center |
| `451c4dfe-de2a-496f-a761-b5546debe6d8` | deploy-center-ci-trigger-bearer-auth | xjp-deploy-center |
| `e57ad1fa-fa8a-4946-a5ce-1e88d2ead4ea` | deploy-center-stage-config-requires-stage-project-slug | xjp-deploy-center |
| `c54d9eb5-36ab-41ea-bfeb-e52049cfbc74` | missiond-oauth-expire-detection-flow-memory | missiond (daemon 自身 OAuth 检测) |
| `8e256700-6441-4151-8b94-132e433354c8` | policy-ask-gemini-only-for-uncertain-architectural-decisions | generic policy (举例提到 OAuth) |
| `6fa0feee-2b41-4b4d-9717-1804fc2f881e` | self-transfer-assistant-design | missiond/jarvis (复用 xjp-auth QR 登录) |

**out count: 25**

---

## Summary

- superseded count: **2** (建议归档；其中 1 条需先拆分)
- promote count: **1** ⚠️ DOMAIN-CONFLICT (`.com` vs `.top` 需用户确认)
- delete count: **1** (建议删除或加 `historical`)
- keep count: **1** (拆分 6a4a7a48 后保留 token storage 约定)
- out count: **25** (跳过，由各自 SSOT 维护)
- **Total: 30** （29 候选 + 1 拆分产生的子条目）

---

## 用户决策点（按优先级）

1. **🔴 P0 域名冲突核实**：`auth.xiaojinpro.com`（KB + 多处消费方）vs `auth.xiaojinpro.top`（lisp ISSUER `:example`）—— 哪个是 production？要不要 promote 一条 `(constants (subdomain auth "..."))` 到 lisp？
2. **🟡 P1 拆分 6a4a7a48**：核心 architecture 部分 superseded 归档；token storage 部分（CutHub 混合存储）建议迁移到 cuthub 项目 KB。
3. **🟡 P1 删除 f6589b56** auth-mysql-to-postgres-migration-plan：迁移已完成，是否删除还是加 `historical` 标签？
4. **🟢 P2 归档 4fa5ed15** xjp-auth-wechat-service-oauth-flow：建议加 `superseded-by-lisp` 标签后归档。
5. **🟢 P3 沉淀 ADR 缺口**：未来 auth 相关 incident/bugfix 会话主动触发 KB 提取（refresh token reuse / Cedar PDP timeout / WeChat HMAC failure / auth_codes 并发等）。

> 删除/拆分操作均**不在本次主控权限内**，等待用户拍板后由用户或主控发起 `mission_kb_mutate` 写操作。

---

## Methodology / Audit Trail

- KB 扫描：6 轮 mission_kb_query (search + list)，关键词覆盖 auth/oauth/jwt/session/cookie/wechat/qr/google/sms/iam/cedar/tenant/auth.xiaojinpro/port 8081/jwks/dcr/device flow/pkce/secret-store/masterkey/login/401/token/refresh/multi-tenant/admin/break_glass。共 ~110 条 raw hits，去重后 29 条候选。
- SSOT 阅读：6 个 lisp 文件 1099 行全文加载并 cross-reference。
- 写操作：仅本报告 `.tmp/auth_kb_review_report.md` 1 处。**未对 KB 执行 mutate/remember/ops 任何写操作。**
- Swarm 失败原因：3 个 ClaudeCode worker 全部被 mission_swarm_run 误路由到 gemini-ultra slot（pool_hint=claude-code-default 但实际派给 gemini），且 gemini 工位 (a) 未挂 mission_kb_query (b) workspace 锁 missiond/ 读不到 xiaojinpro-backend (c) plan mode 不能 Bash mkdir。3 个子任务在 board 上 status=blocked，已留 note 说明。

