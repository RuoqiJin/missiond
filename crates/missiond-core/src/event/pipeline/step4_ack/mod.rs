//! Pipeline step 4 · ack — terminate the synchronous chain and return
//! [`AppendAck`] / [`AppendError`] to the producer.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-4 ack:
//!
//! > 给 producer 回确定的 AppendAck / AppendError,同步链终结
//!
//! ## Zero-component step
//!
//! The frozen lisp explicitly calls out:
//!
//! > no-extra-hop "step 4 没有新组件 — 只是 step 3 的 out-bound 封装"
//!
//! So there is deliberately **no runtime logic here**. The mechanism is
//! [`tokio::sync::oneshot`]: step-3's writer task holds the
//! `oneshot::Sender<Result<AppendAck, AppendError>>` (field
//! `PendingAppend::ack`) and calls `.send(result)` when the batch commits.
//! The producer's `log.append()` call awaits the matching receiver and
//! returns the final `Result`.
//!
//! ## Why a module?
//!
//! Keeping step-4 as a named module makes the 7-step layout readable at
//! a glance — the lisp's `execution-flow` has seven named stops and the
//! code layout should mirror that. If the ack transport ever grows (e.g.
//! multiple phase acks, at-most-once variants, or structured tracing hooks),
//! the implementation would land here without shaking the name.
//!
//! ## Semantic anchors
//!
//! The public ack / error enums live at [`crate::event::log`]:
//!
//! | Type                              | Variant              | When returned (by step-3) |
//! |-----------------------------------|----------------------|---------------------------|
//! | [`crate::event::log::AppendAck`]  | `Committed`          | Normal durable append.    |
//! | [`crate::event::log::AppendAck`]  | `Volatile`           | `ephemeral=true` fast-path (no DB round-trip). |
//! | [`crate::event::log::AppendAck`]  | `AlreadyExists`      | dedupe-key collision resolved via SELECT. |
//! | [`crate::event::log::AppendError`]| `Backpressure`       | `try_send` → channel full. |
//! | [`crate::event::log::AppendError`]| `CausalLoop`         | step-1 guard rejection. |
//! | [`crate::event::log::AppendError`]| `LogUnavailable`     | writer in failed state. |
//! | [`crate::event::log::AppendError`]| `Serialize`/`Other`  | step-2 payload error / misc. |
//!
//! See also [`ack_transport`] for the oneshot wrapper pattern.

pub mod ack_transport;

// Re-export the public surface so consumers can `use
// crate::event::pipeline::step4_ack::{AppendAck, AppendError}`.
pub use crate::event::log::{AppendAck, AppendError, Seq, SpanContext};
pub use ack_transport::AckSender;
