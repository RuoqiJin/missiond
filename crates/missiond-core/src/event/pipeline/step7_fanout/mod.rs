//! Pipeline step 7 · fanout — per-topic broadcast to live subscribers.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-7 fanout:
//!
//! > 按 domain 送到对应 Topic<T> broadcast channel,出口到 4.3
//!
//! Two sub-components map 1:1 to the frozen lisp `topic-registry` and
//! `per-topic-fanout` components:
//!
//! * [`topic`] — one `broadcast::Sender<Arc<T>>` per [`DomainEvent`] type.
//!   Capacity is [`TOPIC_BUFFER_SIZE`] (1024) per frozen lisp §4.2 step-7
//!   `topic-registry.buffer-size`.
//! * [`registry`] — `HashMap<Domain, Arc<dyn AnyTopic>>` + `TypeId` lookup;
//!   the dispatcher tail loop calls `any_topic(domain).fanout_json(payload)`
//!   per event.
//!
//! Slow-subscriber isolation is a direct consequence of `tokio::sync::broadcast`:
//! one lagging receiver surfaces `Lagged` locally without stalling the
//! sender or peers (frozen lisp §4.2 step-7 per-topic-fanout.isolation).
//!
//! Egress (phase-2-live in frozen lisp §4.3) picks up from here: subscribers
//! call `dispatcher.topic::<T>().subscribe()` to obtain a `broadcast::Receiver<Arc<T>>`.
//!
//! [`DomainEvent`]: crate::event::DomainEvent

pub mod registry;
pub mod topic;

pub use registry::{AnyTopic, TopicRegistry, TopicRegistryBuilder, TypedTopic};
pub use topic::Topic;

/// Default broadcast buffer per topic. Frozen lisp §4.2 step-7
/// `topic-registry.buffer-size = 1024`.
pub const TOPIC_BUFFER_SIZE: usize = 1024;
