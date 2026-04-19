//! Event-bus pipeline — 7 ordered steps from producer `append()` to
//! subscriber `broadcast`.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 core:
//!
//! > 事件从 append 到 fan-out 的 7 步流水线 — 模块从上到下对应执行顺序,可观测
//!
//! ```text
//! ← append(event, opts)        (4.1 ingress)
//!   │
//!   ├─ step1_guard              causation check + type resolve
//!   ├─ step2_decide             claim-check threshold + ephemeral/durable
//!   ├─ step3_commit             batch INSERT + seq alloc + dedup (sync)
//!   ├─ step4_ack                → Ok(AppendAck) / Err(AppendError)
//!   ├ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  sync / async boundary
//!   ├─ step5_tail               long-poll event_log for new seq
//!   ├─ step6_gate               drop paused domains
//!   └─ step7_fanout             broadcast to Topic<T> subscribers
//!     │
//!     → (4.3 egress subscribers)
//! ```
//!
//! Invariants (frozen lisp §4.2 core.invariants):
//!
//! * `sync-chain`    — steps 1-4 run inside the producer's `append()` call
//!   stack; `Ok(Committed)` returns **iff** the commit landed.
//! * `async-chain`   — steps 5-7 run in an independent Dispatcher task;
//!   Dispatcher crashes do not block `append()`.
//! * `idempotency`   — step-3 dedup guarantees producer-retry semantics.
//!
//! Each `step{N}_*` module mirrors the lisp section of the same name. See
//! the per-module docs for sub-component breakdowns.

pub mod step1_guard;
pub mod step2_decide;
pub mod step3_commit;
pub mod step4_ack;
pub mod step5_tail;
pub mod step6_gate;
pub mod step7_fanout;
