//! Rate-limit combinator — caps delivery rate; excess events wait rather
//! than being dropped.

use std::time::Duration;

use tokio::time::Instant;

use super::super::super::event_trait::DomainEvent;
use super::super::super::log::Seq;
use super::super::{Ack, Subscription};

pub struct RateLimitedSubscription<T: DomainEvent> {
    pub(super) inner: Subscription<T>,
    pub(super) interval: Duration,
    pub(super) last_emit: Option<Instant>,
}

impl<T: DomainEvent> RateLimitedSubscription<T> {
    pub async fn next(&mut self) -> Option<Ack<T>> {
        let ack = self.inner.next().await?;

        if let Some(last) = self.last_emit {
            let now = Instant::now();
            let elapsed = now.saturating_duration_since(last);
            if elapsed < self.interval {
                tokio::time::sleep(self.interval - elapsed).await;
            }
        }
        self.last_emit = Some(Instant::now());
        Some(ack)
    }

    pub async fn cursor(&self) -> Seq {
        self.inner.cursor().await
    }
}
