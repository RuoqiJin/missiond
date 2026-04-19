//! Ack transport — `oneshot` wrapper used by step-3 commit to reply to
//! step-4 ack.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-4 ack
//! ack-transport:
//!
//! > mechanism     "oneshot::Sender<Result<AppendAck, AppendError>>"
//! > return-timing "step 3 batch 落盘后立刻回每一个 pending oneshot"
//! > ephemeral-path "ephemeral 事件 skip DB 语义下仍 append 成功 → 回
//!                   Ok(Volatile{seq})"
//! > no-extra-hop "step 4 没有新组件 — 只是 step 3 的 out-bound 封装"
//!
//! # Implementation anchor
//!
//! The concrete oneshot lives on [`crate::event::pipeline::step3_commit::log_writer::PendingAppend`]
//! (field `ack`). The writer task fills the oneshot at three call sites
//! inside `LogWriter::flush`:
//!
//! * Happy path: `ack.send(Ok(AppendAck::Committed { seq, durable: true }))`.
//! * Dedup hit: `ack.send(Ok(AppendAck::AlreadyExists { seq }))`.
//! * Failed state / backend error:
//!   `ack.send(Err(AppendError::LogUnavailable(msg)))`.
//!
//! The ephemeral fast-path short-circuits in `LogWriterHandle::append` before
//! ever queueing a `PendingAppend`, returning
//! `Ok(AppendAck::Volatile { seq })` directly — the writer task is not
//! involved.
//!
//! This type alias exists so callers that want to be **loud about the
//! contract** can write `AckSender` instead of the raw
//! `tokio::sync::oneshot::Sender<Result<..>>`.

use crate::event::log::{AppendAck, AppendError};

/// The oneshot sender the writer uses to reply to an individual
/// [`crate::event::pipeline::step3_commit::log_writer::PendingAppend`].
pub type AckSender = tokio::sync::oneshot::Sender<Result<AppendAck, AppendError>>;

/// Convenience: build a matched `(AckSender, AckReceiver)` pair. The step-3
/// code already inlines `oneshot::channel()`; callers that prefer an
/// explicit name can use this.
pub fn new_ack_channel() -> (AckSender, tokio::sync::oneshot::Receiver<Result<AppendAck, AppendError>>) {
    tokio::sync::oneshot::channel()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::log::Seq;

    #[tokio::test]
    async fn new_ack_channel_delivers_committed() {
        let (tx, rx) = new_ack_channel();
        let _ = tx.send(Ok(AppendAck::Committed {
            seq: Seq(1),
            durable: true,
        }));
        let got = rx.await.expect("recv ok");
        assert_eq!(got.unwrap().seq(), Seq(1));
    }

    #[tokio::test]
    async fn dropping_sender_surfaces_as_recv_err() {
        let (tx, rx) = new_ack_channel();
        drop(tx);
        assert!(rx.await.is_err());
    }
}
