//! Filter combinator — pass-through predicate. Dropped events are silently
//! acked so the cursor keeps making progress (use [`FailurePolicy`] if you
//! want failure routing instead).
//!
//! [`FailurePolicy`]: super::super::options::FailurePolicy

use super::super::super::event_trait::DomainEvent;
use super::super::super::log::Seq;
use super::super::{Ack, Subscription};

pub struct FilteredSubscription<T: DomainEvent, F>
where
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    pub(super) inner: Subscription<T>,
    pub(super) pred: F,
}

impl<T: DomainEvent, F> FilteredSubscription<T, F>
where
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    pub async fn next(&mut self) -> Option<Ack<T>> {
        loop {
            let ack = self.inner.next().await?;
            if (self.pred)(ack.event()) {
                return Some(ack);
            }
            ack.silent_ack().await;
        }
    }

    pub async fn cursor(&self) -> Seq {
        self.inner.cursor().await
    }
}
