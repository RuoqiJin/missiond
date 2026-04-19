//! Pluggable live source — abstracts where a [`super::Subscription`] gets
//! live events from after it transitions to [`super::lifecycle::Phase::Live`].
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.3 subscription-
//! live-source. The trait keeps the [`super::lifecycle::Lifecycle`] driver
//! independent of the specific transport; production wraps the
//! dispatcher's `tokio::sync::broadcast::Receiver<Arc<T>>` via
//! [`BroadcastLiveSource`], while unit tests drive it with an mpsc via
//! [`MpscLiveSource`].

use std::sync::Arc;

use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver as BroadcastReceiver;

use super::super::event_trait::DomainEvent;
use super::super::log::Seq;
use super::lifecycle::LifecycleError;

/// Pluggable live source. In production this wraps a
/// `tokio::sync::broadcast::Receiver<Arc<T>>` from the dispatcher topic; in
/// tests a `tokio::sync::mpsc::Receiver<Arc<T>>` is convenient.
#[async_trait::async_trait]
pub trait LiveSource<T>: Send + Sync
where
    T: DomainEvent,
{
    /// Receive one live event. Returns:
    ///   * `Ok(Some(arc))` — a new event arrived.
    ///   * `Ok(None)`       — the channel closed; subscription should terminate.
    ///   * `Err(LifecycleError::LiveLagged { .. })` — overflow: caller flips
    ///     back to bootstrap.
    async fn recv(&mut self) -> Result<Option<Arc<T>>, LifecycleError>;
}

/// Adapter from `tokio::sync::broadcast::Receiver<Arc<T>>` to [`LiveSource`].
pub struct BroadcastLiveSource<T>
where
    T: DomainEvent,
{
    rx: BroadcastReceiver<Arc<T>>,
}

impl<T: DomainEvent> BroadcastLiveSource<T> {
    pub fn new(rx: BroadcastReceiver<Arc<T>>) -> Self {
        Self { rx }
    }
}

#[async_trait::async_trait]
impl<T: DomainEvent> LiveSource<T> for BroadcastLiveSource<T> {
    async fn recv(&mut self) -> Result<Option<Arc<T>>, LifecycleError> {
        match self.rx.recv().await {
            Ok(ev) => Ok(Some(ev)),
            Err(RecvError::Closed) => Ok(None),
            Err(RecvError::Lagged(n)) => Err(LifecycleError::LiveLagged {
                skipped: n,
                cursor: Seq(0), // filled in by caller that knows the cursor
            }),
        }
    }
}

/// Live-adapter backed by an mpsc channel. Used by unit tests.
pub struct MpscLiveSource<T>
where
    T: DomainEvent,
{
    rx: tokio::sync::mpsc::Receiver<Arc<T>>,
}

impl<T: DomainEvent> MpscLiveSource<T> {
    pub fn new(rx: tokio::sync::mpsc::Receiver<Arc<T>>) -> Self {
        Self { rx }
    }
}

#[async_trait::async_trait]
impl<T: DomainEvent> LiveSource<T> for MpscLiveSource<T> {
    async fn recv(&mut self) -> Result<Option<Arc<T>>, LifecycleError> {
        Ok(self.rx.recv().await)
    }
}
