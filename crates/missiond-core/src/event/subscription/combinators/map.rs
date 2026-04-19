//! Map combinator — pure transform. Seq/attempt are preserved and routed
//! through the source subscription's flush plumbing via [`super::wrap_ack`].

use std::sync::Arc;

use super::super::super::event_trait::DomainEvent;
use super::super::{Ack, Subscription};
use super::wrap_ack;

pub struct MappedSubscription<T: DomainEvent, U: DomainEvent, F>
where
    F: Fn(&T) -> U + Send + Sync + 'static,
{
    pub(super) inner: Subscription<T>,
    pub(super) map_fn: F,
    pub(super) _p: std::marker::PhantomData<U>,
}

impl<T: DomainEvent, U: DomainEvent, F> MappedSubscription<T, U, F>
where
    F: Fn(&T) -> U + Send + Sync + 'static,
{
    pub async fn next(&mut self) -> Option<Ack<U>> {
        let ack = self.inner.next().await?;
        let seq = ack.seq();
        let attempt = ack.attempt();
        let mapped = (self.map_fn)(ack.event());

        let shared = self.inner.shared();
        let flush_tx = self.inner.flush_signal_tx();
        // Silent-ack the source so the map downstream ack controls
        // cursor advancement — otherwise dropping a mapped ack would
        // never touch the source cursor.
        ack.silent_ack().await;

        Some(wrap_ack(Arc::new(mapped), seq, attempt, shared, flush_tx))
    }
}
