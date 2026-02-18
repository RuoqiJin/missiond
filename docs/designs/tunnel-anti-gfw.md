# 隧道抗 GFW 加固 — 执行方案

## 背景

GFW 于 2026-02-18 封锁 `34.104.147.118:9876` 明文 WS 隧道。已紧急迁移 WSS+443 (Caddy 反代)。
本方案在此基础上分层加固，目标：让隧道流量与普通 Chrome HTTPS 不可区分。

## 代码位置

| 文件 | 角色 |
|------|------|
| `src/services/tunnel/client.rs` | 隧道客户端 (privatecloud/Windows) |
| `src/services/tunnel/server.rs` | 隧道服务端 (GCP) |
| `src/api/tunnel.rs` | WS 握手端点 `/tunnel/ws` |
| `src/services/tunnel/mod.rs` | 模块入口 |
| `Cargo.toml` | 依赖 (当前 rustls) |

## 当前状态 (改动前)

| 项 | 现状 | 问题 |
|----|------|------|
| 认证失败 | 返回 `401 "Invalid tunnel token"` | 暴露端点存在 |
| 重连间隔 | 固定 5s | 规律性重连可被统计识别 |
| 心跳间隔 | 固定 30s | 精确周期是机器人特征 |
| 流量 | 无 padding，空闲无流量 | 包大小分布异常 |
| TLS | rustls | JA3 指纹与 Chrome 不同 |

---

## P0 — 沉默原则 (2h)

**目标**：未认证请求看不出这是隧道端点

### 改动

**`src/api/tunnel.rs:181-182`**

```rust
// BEFORE
if token != Some(&state.tunnel_auth_token) {
    return (StatusCode::UNAUTHORIZED, "Invalid tunnel token").into_response();
}

// AFTER
if token != Some(&state.tunnel_auth_token) {
    return (StatusCode::NOT_FOUND, "").into_response();
}
```

**效果**：主动探测者访问 `/tunnel/ws` 无 token → 404，与任何不存在的路径一致。Caddy 前置，真实网站兜底。

---

## P1 — 行为特征消除 (3h)

**目标**：消除固定间隔的机器人指纹

### 1.1 重连指数退避 + jitter

**`src/services/tunnel/client.rs`**

删除常量：
```rust
// DELETE
const RECONNECT_DELAY_SECS: u64 = 5;
```

新增函数：
```rust
/// 计算重连延迟：指数退避 + jitter
/// 基础 5s, 倍数 3x, 上限 300s, jitter ±30%
fn reconnect_delay(attempt: u32) -> Duration {
    use rand::Rng;
    let base_secs = 5.0 * 3.0_f64.powi(attempt.min(5) as i32);
    let capped = base_secs.min(300.0);
    let jitter = rand::thread_rng().gen_range(0.7..1.3);
    Duration::from_secs_f64(capped * jitter)
}
```

重连循环改为：
```rust
let mut attempt: u32 = 0;

loop {
    // ... run_client ...
    match result {
        Ok(ClientResult::Disconnected) => { attempt += 1; }
        Err(_) => { attempt += 1; }
        // Shutdown/Takeover → break
    }

    let delay = reconnect_delay(attempt);
    info!(delay_ms = delay.as_millis(), attempt, "Reconnecting");

    tokio::select! {
        _ = tokio::time::sleep(delay) => {}
        _ = shutdown_token.cancelled() => break;
    }
}
// 成功连接后 reset: attempt = 0 (在 run_client 内 connected=true 时)
```

### 1.2 心跳 jitter

**`src/services/tunnel/client.rs`** 和 **`server.rs`**

```rust
// 替换固定 interval
// BEFORE: let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
// AFTER:
fn next_ping_delay() -> Duration {
    use rand::Rng;
    let base = 30.0;
    let jitter = rand::thread_rng().gen_range(0.7..1.3);
    Duration::from_secs_f64(base * jitter)
}
// 在 select! 中用 tokio::time::sleep(next_ping_delay()) 替代 ping_interval.tick()
```

### 依赖

Cargo.toml 加 `rand = "0.8"`

---

## P2 — 流量混淆 (1-2 天)

**目标**：打乱包大小分布，消除空闲静默期

### 2.1 随机 padding ✅

**实际实现**：WebSocket 层 padding（不修改 codec 帧格式，向后兼容）

`src/services/tunnel/codec.rs` 新增 `TunnelCodec::pad()` / `try_unpad()`：

```rust
/// 格式: [pad_len: 1B][binary_frame][random: pad_len bytes]
/// pad_len 4-64，首字节 ≤ 64 (0x40)
/// 未 padding 帧首字节 0xDA (218)，因此可自动区分
pub fn pad(frame: &[u8]) -> Vec<u8>;
pub fn try_unpad(data: &[u8]) -> Option<&[u8]>;
```

`decode_message` 按优先级尝试：直接帧 → unpad → JSON fallback。

**兼容性**：无需协商。decoder 自动检测两种格式。升级窗口期（~1h）部分消息可能丢失，重连后恢复。

### 2.2 空闲伪流量 ✅

`TunnelMessage::Noise { data: Vec<u8> }` + codec `Noise = 28`

```rust
// 随机 60-180s 发一个 Noise 帧 (16-128 bytes 随机数据)
_ = &mut fake_sleep => {
    if use_binary.load(Ordering::Relaxed) {
        let noise = generate_noise();
        let ws_msg = encode_message(&noise, true)?;
        let _ = ws_tx.send(ws_msg).await;
    }
    fake_sleep.as_mut().reset(...);
}
```

接收方在 select! 循环中 `matches!(Noise) → continue` 直接丢弃。

---

## P3 — TLS 指纹伪装 (3-5 天)

**目标**：JA3 指纹与 Chrome 一致

### 方案：rustls → boring-ssl

```toml
# Cargo.toml
# BEFORE
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }

# AFTER
tokio-tungstenite = { version = "0.24", features = ["native-tls"] }
reqwest = { version = "0.12", features = ["json", "native-tls"] }
boring = "4"
tokio-boring = "4"
```

然后用 `boring` 的 `SslConnector` 手动配置 cipher suite 顺序、ALPN、extensions，模拟 Chrome 的 Client Hello。

### 关键配置项

```rust
use boring::ssl::{SslConnector, SslMethod};

let mut builder = SslConnector::builder(SslMethod::tls_client())?;
// Chrome 131 cipher suites (按 Chrome 顺序)
builder.set_cipher_list(
    "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:\
     ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:..."
)?;
// ALPN
builder.set_alpn_protos(b"\x02h2\x08http/1.1")?;
```

### 交叉编译注意

boring-ssl 需要 C 编译器。Linux musl 交叉编译需确保 CI 环境有 `musl-gcc` + `cmake`。
Windows 交叉编译需要 MSVC 兼容工具链。

**建议**：P3 单独分支开发，CI 验证通过后再合入。

---

## 实施顺序

| 阶段 | 内容 | 预估 | 发版 |
|------|------|------|------|
| **P0+P1** | 沉默 + 行为消除 | 半天 | v0.9.46 ✅ |
| **P2** | padding + 伪流量 | 半天 | v0.9.47 ✅ |
| **P3** | boring-ssl TLS 指纹 | 3-5 天 | v0.9.48 |

P0+P1 合并一个版本发布 — 改动小、风险低、收益大（消除 80% 可识别特征）。

## 注意事项

- P0+P1 仅改客户端，GCP server 侧仅改 401→404 和心跳 jitter
- P2 需 client/server 同步升级，先升 server (GCP) 再升 client
- P3 影响 CI 构建链（boring-ssl 依赖 C 编译），需先验证交叉编译
- 每个阶段发版后用 `xjp_update_chain_status` 验证全链路
