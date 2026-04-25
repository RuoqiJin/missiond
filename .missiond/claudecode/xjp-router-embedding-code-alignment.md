# MissionD v2 Code Alignment Task: xjp-router Embedding Provider

请按 MissionD v2 Lisp 架构做代码同构：把 embedding provider 从 Sonnet/旧 gateway 切到 `xjp-router` typed HTTP client。

只做代码同构，不重新设计架构，不修改 `.missiond/v2/*.lisp`。当前 Lisp 是工作树里的最新设计，请先读这些锚点：

- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-worker.lisp` :: `section xjp-router-gateway`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-worker.lisp` :: `xjp-router-client-bootstrap`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-worker.lisp` :: `xjp-router-embedding`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F7-embedding-pipeline`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-tools.lisp` :: `mission_embedding_ops`

## Parallel Scope

This task should mostly touch the LLM/embedding lane:

- `crates/missiond-daemon/src/llm/**`
- embedding worker / embedding gateway call sites
- daemon config/env loading for xjp-router endpoint/token
- tests for the new client and embedding path

Avoid touching MCP tool registration files unless strictly necessary. This should be safe to run in parallel with project-root-spawn-cwd work.

## Goals

1. Add a typed `xjp_router_client` module, preferably at:
   - `crates/missiond-daemon/src/llm/xjp_router_client.rs`
2. Config / env:
   - endpoint: `MISSION_XJP_ROUTER_ENDPOINT` or existing config equivalent if one already exists
   - auth token: `MISSION_XJP_ROUTER_AUTH_TOKEN` or existing secret source if one already exists
   - model: default `qwen3` unless codebase has a more precise embedding model convention
3. Embedding path:
   - route embedding calls through `xjp_router_client.embed(texts)`
   - use HTTP `POST /embed` or the actual xjp-router contract if already documented in repo
   - return typed vectors and surface provider errors clearly
4. Fail fast:
   - no fallback to `sonnet_gateway` for embedding
   - if endpoint/token missing and embedding is requested, return structured error
5. Keep Sonnet chat responsibilities unchanged.
6. Add provider lifecycle logging; do not add new event-bus variants in this task unless they already exist.

## Non-Goals

- Do not implement xjp-router chat or rerank.
- Do not modify `.missiond/v2/*.lisp`.
- Do not redesign config layout if a minimal env/config hook is enough.
- Do not add a fallback provider.

## Acceptance

- `cargo build --workspace`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-core --lib`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

Add focused tests where practical:

- request serialization for embed batch
- response parsing into vectors
- missing endpoint/token returns a clear error
- embedding path does not call Sonnet fallback

## Deliverables

- List modified files.
- State whether xjp-router embedding is full / partial / config-only.
- State the exact env/config names used.
- State any runtime assumption about the external xjp-router `/embed` API.
