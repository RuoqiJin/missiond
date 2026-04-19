//! Debounce combinator — fixed-window; last event in the window wins.
//!
//! Earlier events are silently acked so the cursor keeps advancing.

use std::time::Duration;

use tokio::time::{self, Instant};

use super::super::super::event_trait::DomainEvent;
use super::super::super::log::Seq;
use super::super::{Ack, Subscription};

pub struct DebouncedSubscription<T: DomainEvent> {
    pub(super) inner: Subscription<T>,
    pub(super) window: Duration,
}

impl<T: DomainEvent> DebouncedSubscription<T> {
    pub async fn next(&mut self) -> Option<Ack<T>> {
        // Wait for the first event — without an initial event there's
        // nothing to debounce.
        let mut last = self.inner.next().await?;
        let deadline = Instant::now() + self.window;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Some(last);
            }
            match time::timeout(remaining, self.inner.next()).await {
                Ok(Some(new_ack)) => {
                    // New event replaces previous — silently ack the old
                    // one so the cursor advances.
                    last.silent_ack().await;
                    last = new_ack;
                }
                Ok(None) => return Some(last),
                Err(_) => return Some(last),
            }
        }
    }

    pub async fn cursor(&self) -> Seq {
        self.inner.cursor().await
    }
}
