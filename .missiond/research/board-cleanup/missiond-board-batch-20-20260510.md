# MissionD Board Cleanup Batch 20 - 2026-05-10

Scope: read-only cleanup review for dispatch BoardTask `7214ed35-fdae-45ad-a99d-0724838fe916`.

Reviewed BoardTasks:

- `d47d7800-b846-456c-b526-b1774d19ee17`
- `2bdb1b50-107b-4fd9-89db-baf06806a377`
- `5308daf1-fba6-49a4-99e4-fd53f3366fb7`
- `f411c3b8-9798-466d-b27d-0b40c84558f3`
- `e7448549-6915-4c1b-af1c-f7e2c28fcdf2`

Board evidence from `mission_board_query` in this review: all five reviewed tasks are `status=open`; no reviewed task has Board notes. I did not mutate Board status or notes.

## Findings

| Task ID | Title | Classification | Finding |
| --- | --- | --- | --- |
| `d47d7800-b846-456c-b526-b1774d19ee17` | `Investigate resident Codex CLI slot-to-thread attribution` | `close-covered` | The Codex CLI slot/thread/task binder is now specified, implemented, and checker-pinned. |
| `2bdb1b50-107b-4fd9-89db-baf06806a377` | `Auth: pin canonical smoke route paths in M6 context-pack` | `keep` | The concrete ask is still unfilled: health/OIDC/JWKS URLs exist, but login/SMS/email/admin smokes remain human-language strings and no expected status codes are pinned. |
| `5308daf1-fba6-49a4-99e4-fd53f3366fb7` | `像 GPU 一样喷出 Rust — Lisp 元编程探索` | `close-superseded` | The old broad research row is superseded by the current Forge / Lisp-code-sync surfaces; any next work should be a narrow Forge task, not this MissionD research stub. |
| `f411c3b8-9798-466d-b27d-0b40c84558f3` | `MissionD 架构升级 Phase 2: 并发与事件驱动` | `close-covered` | Phase 2's concurrency/event/reconcile/DB goals are covered by current Postgres-only runtime, EventBus infrastructure, reconcile workers, and batch/idempotent writes. |
| `e7448549-6915-4c1b-af1c-f7e2c28fcdf2` | `MissionD 架构升级 Phase 3: 企业级能力` | `close-stale` | The original enterprise umbrella no longer matches current Phase 3 architecture; some security/permission pieces exist, but tonic/bollard/OpenTelemetry/secrecy dependencies are absent and the direction shifted to V3 shared memory / sysinfra control. |

## Evidence

### `d47d7800-b846-456c-b526-b1774d19ee17` - `close-covered`

Original ask: distinguish resident Codex CLI from Codex Desktop/current user threads and establish a concrete slot-to-thread/task binder contract before code changes.

Current evidence:

- `.missiond/v3/missiond-blueprint.lisp:1768-1803` now pins canonical `codex_cli` source handling, placeholder fallback, raw rollout ingestion, source-state categories, provider-aware classification, and dry-run historical repair.
- `scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs:157-166` requires `classify_conversation_type`, `audit_classification`, `audit_historical_classification`, and explicit backfill actions.
- `scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs:452-470` requires Codex ingestion to call the classifier, preserve `raw_role`, use `get_running_slot_task`, refresh message counts, and rejects the old `conversation_type: "user"` / `raw_role: None` pattern.
- `crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs:318-326` reads Codex sqlite plus raw rollout JSONL, keeping `session_meta.payload.id` as the canonical conversation id.
- `crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs:769-789` reads `slot_id` from the session, classifies via `classify_conversation_type`, and stamps the currently running slot task into `Conversation.task_id`.
- `crates/missiond-core/src/db/conversation_query.rs:32-50` implements the provider-aware classifier: slot-bound Codex -> `worker`, unbound Codex -> `codex_chat`.
- Verification command: `node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs` -> `v3 CLI conversation ingestion isomorphism check OK`.

Conclusion: the investigation task is now covered by SSOT, code, and checker. Close this child task; do not close its parent unless the parent has separate remaining scope.

### `2bdb1b50-107b-4fd9-89db-baf06806a377` - `keep`

Original ask: pin canonical Auth smoke route paths and expected status codes in the M6 deploy context pack / successor packs.

Current evidence:

- `.missiond/v3/runtime/master-control/context-packs/auth-m6-production-deploy-20260507.lisp:80-83` says the worker must run post-deploy health/OIDC/JWKS/login-route and consumer compatibility smoke.
- `.missiond/v3/runtime/master-control/context-packs/auth-m6-production-deploy-20260507.lisp:84-93` has the `:post-deploy-smoke` vector.
- Lines `85-87` already contain concrete URLs for:
  - `https://auth.xiaojinpro.com/health/ready`
  - `https://auth.xiaojinpro.com/.well-known/openid-configuration`
  - `https://auth.xiaojinpro.com/.well-known/jwks.json`
- Lines `88-93` are still not canonical route specs: `Google login route regression`, `WeChat callback route regression`, `phone OTP route regression`, `email/password route regression`, runtime tenant/application/product registration, and consumer JWT compatibility are prose, not URL + expected status pairs.
- Search evidence: `rg "auth/google/authorize|auth/wechat|auth/sms/send|auth/email/login" .missiond/v3/runtime/master-control/context-packs/auth-m6-production-deploy-20260507.lisp` returned no matches.
- `.missiond/v3/missiond-blueprint.lisp:1620` pins only the generic health endpoints for auth service runtime universe; it does not cover the login/SMS/email/admin smoke matrix requested by this BoardTask.

Conclusion: keep. The original ask still holds and is concrete enough. Recommended future edit: replace the prose entries with structured URL / method / expected status / acceptable negative-path behavior entries, or promote them into a durable Auth smoke SSOT and project them into deploy packs.

### `5308daf1-fba6-49a4-99e4-fd53f3366fb7` - `close-superseded`

Original ask: explore a Lisp/metaprogramming layer that can generate optimized Rust, with Racket-to-Rust / proc-macro DSL style research.

Current evidence:

- `crates/missiond-mcp/src/tools/compute/forge.rs:7-17` exposes `mission_forge_build`: "Run Forge Lisp->Rust stamping for a registered project" and supports dry-run preview.
- `crates/missiond-mcp/src/tools/compute/forge.rs:20-30` exposes `mission_forge_lint` for Forge governance lint over project `intent.lisp`.
- `crates/missiond-mcp/src/tools/knowledge/board.rs:1-2` is a generated Rust tool definition with `// GENERATED BY FORGE - DO NOT EDIT` and source `.missiond/intent-tools.lisp`.
- `.missiond/v3/missiond-blueprint.lisp:3164-3176` defines the current `lisp-code-sync-loop` as code-aligned: watches `.missiond` authoring paths, compiles, runs code-isomorphism gates, writes bounded reports, and creates deduped BoardTasks for failing gates.
- `scripts/check-v3-lisp-code-sync-isomorphism.mjs:38-85` pins the lisp-code-sync workflow, code-isomorphism gate, exact accepted shard requirement, runtime report paths, and stale runtime task revalidation.
- Search evidence found this BoardTask mainly in historical memory review exports, not in a current exact implementation plan.
- Verification command: `node scripts/check-v3-lisp-code-sync-isomorphism.mjs` -> `v3 lisp-code-sync isomorphism check OK`.

Conclusion: the broad research row has been superseded by concrete Forge and V3 Lisp/code-sync mechanisms. Close as superseded. If new work is desired, write a fresh narrow Forge experiment with explicit input Lisp, expected Rust output, and checker.

### `f411c3b8-9798-466d-b27d-0b40c84558f3` - `close-covered`

Original ask: Phase 2 concurrency/event-driven architecture, including DB async/pooling, EventBus, restart reconciliation, and batch/CQRS-style writes.

Current evidence:

- `docs/designs/architecture-upgrade-phase2.md:360-388` records the old Phase 2 review and says Phase 2 deliberately avoided over-expanding into Event Sourcing / Actor model at that time.
- `Cargo.toml:39-44` states MissionD runtime DB is PostgreSQL-only; `rusqlite` remains only for provider-local readers and independent crates, while `sqlx` is configured for Postgres.
- `crates/missiond-core/Cargo.toml:13-16` defaults to the `postgres` feature and wires it to `dep:sqlx`.
- `scripts/check-v3-ops-infra-isomorphism.mjs:300-305` pins the invariant that MissionD primary runtime DB is PostgreSQL-only and old SQLite runtime backend / migration / sqlite feature cfg must be absent.
- `crates/missiond-daemon/src/bus/bootstrap.rs:130-188` bootstraps the current bus services over `PgPool`: Postgres blob store, log writer, cursor store, DLQ, dispatcher, control gate, and Postgres tail source.
- `crates/missiond-daemon/src/workers/local/reconcile_worker.rs:1-10` documents the daily JSONL-to-DB integrity checker using `insert_conversation_messages_batch()` with `ON CONFLICT DO NOTHING`.
- `crates/missiond-daemon/src/workers/local/reconcile_worker.rs:47-68` runs periodic reconciliation and exposes `run_reconciliation_now`.
- `crates/missiond-core/src/db/pg/message.rs:46-63` implements `insert_conversation_messages_batch` with idempotent `ON CONFLICT (message_uuid) DO NOTHING`.
- Verification command: `node scripts/check-v3-ops-infra-isomorphism.mjs` -> `v3 ops-infra Lisp/code isomorphism check OK`.

Conclusion: the old Phase 2 task is covered by the current architecture and checker suite. Close.

### `e7448549-6915-4c1b-af1c-f7e2c28fcdf2` - `close-stale`

Original ask: Phase 3 enterprise umbrella, described in the BoardTask as tracing/OpenTelemetry, tonic gRPC ComputeProvider, bollard Docker sandboxing, and secrecy/RBAC hardening.

Current evidence:

- `docs/designs/architecture-upgrade-phase3.md:1-11` now describes Phase 3 as async boundary governance plus autopilot/decision-engine splitting, not the older enterprise umbrella.
- Cargo dependency search over `Cargo.toml`, `crates/*/Cargo.toml`, and `packages/*/package.json` for `opentelemetry`, `tonic`, `bollard`, and `secrecy` returned no matches.
- `.missiond/v3/missiond-blueprint.lisp:2585-2594` pins the current durable concurrency direction as `mission-shared-memory`: Postgres `shared_events`, `shared_artifacts`, `shared_claims`, agent cursors, and EventBus wakeup projection.
- `.missiond/v3/missiond-blueprint.lisp:2674-2684` pins current `sysinfra-control`, including `mission_permission_query` and `mission_permission_mutate`.
- `crates/missiond-daemon/src/handlers/mod.rs:52-54` routes `mission_permission_query` and `mission_permission_mutate` to the sysinfra permission handler.
- `crates/missiond-daemon/src/helpers.rs:55-84` enforces owner-only config permissions for sensitive files such as `servers.yaml`, MCP configs, and `config/permissions.yaml`.

Conclusion: do not keep this wide Phase 3 row. It is stale against current architecture and only partially overlaps with present sysinfra/security work. Close stale; if any enterprise item is still desired, open a fresh exact task such as "add OTLP exporter" or "design remote ComputeProvider transport" with one acceptance surface.

## Recommendations

- Close `d47d7800-b846-456c-b526-b1774d19ee17` as `close-covered`.
- Keep `2bdb1b50-107b-4fd9-89db-baf06806a377`; it is a small unfinished Auth smoke-route pinning task.
- Close `5308daf1-fba6-49a4-99e4-fd53f3366fb7` as `close-superseded`.
- Close `f411c3b8-9798-466d-b27d-0b40c84558f3` as `close-covered`.
- Close `e7448549-6915-4c1b-af1c-f7e2c28fcdf2` as `close-stale`.

## Verification

- Wrote only this file: `.missiond/research/board-cleanup/missiond-board-batch-20-20260510.md`.
- Did not run any Board mutation tool; no Board status or note was changed.
- Did not stage or commit.
- Read-only code/SSOT/checker inspection included files under `.missiond`, `crates`, `scripts`, `docs`, and Cargo manifests.
- Checkers run:
  - `node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs` -> OK.
  - `node scripts/check-v3-ops-infra-isomorphism.mjs` -> OK.
  - `node scripts/check-v3-lisp-code-sync-isomorphism.mjs` -> OK.
