//! Topic registry — `Domain -> Box<dyn AnyTopic>` dispatch table.
//!
//! Frozen lisp §4.2.c `topic-registry`:
//!
//! > `type: static HashMap<Domain, Topic<dyn Any>>`
//!
//! Each domain event type `T: DomainEvent` is registered once during
//! [`TopicRegistryBuilder::register`]. At runtime the dispatcher looks up a
//! topic by [`Domain`] (fast path for the tail loop) or by `TypeId` (via
//! [`TopicRegistry::topic::<T>`]).
//!
//! `AnyTopic` erases the generic `T` so the registry can be a plain map.
//! The concrete impl holds a [`Topic<T>`] and knows how to deserialize a
//! JSON payload back into `Arc<T>` before calling `topic.publish`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;
use super::TOPIC_BUFFER_SIZE;
use super::tail::DispatchError;
use super::topic::Topic;

/// Type-erased view of a [`Topic<T>`]. The dispatcher tail loop hands in a
/// JSON payload (from `event_log.payload_inline` or from the claim-check
/// blob store) and the impl deserializes into `Arc<T>` before fan-out.
pub trait AnyTopic: Send + Sync + std::fmt::Debug {
    fn domain(&self) -> Domain;
    fn type_id(&self) -> TypeId;
    fn receiver_count(&self) -> usize;

    /// Deserialize the JSON payload into the concrete `T` and fan-out to
    /// every subscriber.
    ///
    /// Errors out of the tail loop if the payload shape mismatches the
    /// registered type — that's a schema bug and must surface.
    fn fanout_json(&self, payload: &serde_json::Value) -> Result<(), DispatchError>;

    /// Downcast helper used by `TopicRegistry::topic::<T>`.
    fn as_any(&self) -> &dyn Any;
}

/// Concrete `AnyTopic` impl — wraps a [`Topic<T>`] and carries the type id
/// for introspection.
pub struct TypedTopic<T: DomainEvent> {
    topic: Topic<T>,
}

impl<T: DomainEvent> TypedTopic<T> {
    pub fn new(buffer_size: usize) -> Self {
        Self {
            topic: Topic::<T>::new(buffer_size),
        }
    }

    pub fn topic(&self) -> Topic<T> {
        self.topic.clone()
    }
}

impl<T: DomainEvent> std::fmt::Debug for TypedTopic<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedTopic")
            .field("domain", &T::domain())
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T: DomainEvent> AnyTopic for TypedTopic<T> {
    fn domain(&self) -> Domain {
        T::domain()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn receiver_count(&self) -> usize {
        self.topic.receiver_count()
    }

    fn fanout_json(&self, payload: &serde_json::Value) -> Result<(), DispatchError> {
        let event: T = serde_json::from_value(payload.clone()).map_err(|e| {
            DispatchError::PayloadDeserialize {
                domain: T::domain(),
                err: e.to_string(),
            }
        })?;
        self.topic.publish(Arc::new(event));
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Immutable registry consumed by the running dispatcher.
pub struct TopicRegistry {
    by_domain: HashMap<Domain, Arc<dyn AnyTopic>>,
    by_type_id: HashMap<TypeId, Arc<dyn AnyTopic>>,
}

impl TopicRegistry {
    /// Look up the type-erased topic for a given [`Domain`]. Used by the
    /// tail loop after it reads a `LoggedEvent`.
    pub fn any_topic(&self, d: Domain) -> Option<Arc<dyn AnyTopic>> {
        self.by_domain.get(&d).cloned()
    }

    /// Look up the typed topic for a given event type `T`. Used by
    /// subscribers.
    pub fn topic<T: DomainEvent>(&self) -> Option<Topic<T>> {
        let any = self.by_type_id.get(&TypeId::of::<T>())?.clone();
        any.as_any()
            .downcast_ref::<TypedTopic<T>>()
            .map(|t| t.topic())
    }

    /// Count of registered domains.
    pub fn len(&self) -> usize {
        self.by_domain.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_domain.is_empty()
    }

    /// Iterate over `(Domain, Arc<dyn AnyTopic>)` entries — for metrics /
    /// introspection.
    pub fn iter(&self) -> impl Iterator<Item = (Domain, Arc<dyn AnyTopic>)> + '_ {
        self.by_domain.iter().map(|(d, t)| (*d, t.clone()))
    }
}

impl std::fmt::Debug for TopicRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TopicRegistry")
            .field("domains", &self.by_domain.len())
            .finish()
    }
}

/// Mutable builder used during dispatcher boot.
///
/// Calling `.register::<T>()` twice for the same `T` panics (double
/// registration is a configuration bug — we prefer a loud boot-time
/// failure over silent last-write-wins).
pub struct TopicRegistryBuilder {
    by_domain: HashMap<Domain, Arc<dyn AnyTopic>>,
    by_type_id: HashMap<TypeId, Arc<dyn AnyTopic>>,
}

impl TopicRegistryBuilder {
    pub fn new() -> Self {
        Self {
            by_domain: HashMap::new(),
            by_type_id: HashMap::new(),
        }
    }

    pub fn register<T: DomainEvent>(mut self) -> Self {
        let typed = Arc::new(TypedTopic::<T>::new(TOPIC_BUFFER_SIZE)) as Arc<dyn AnyTopic>;
        let domain = T::domain();
        if let Some(existing) = self.by_domain.get(&domain) {
            panic!(
                "topic registry: domain {:?} already registered (type {:?}); cannot re-register for {:?}",
                domain,
                existing.type_id(),
                TypeId::of::<T>()
            );
        }
        if self.by_type_id.contains_key(&TypeId::of::<T>()) {
            panic!(
                "topic registry: type {} already registered",
                std::any::type_name::<T>()
            );
        }
        self.by_domain.insert(domain, typed.clone());
        self.by_type_id.insert(TypeId::of::<T>(), typed);
        self
    }

    pub fn build(self) -> TopicRegistry {
        TopicRegistry {
            by_domain: self.by_domain,
            by_type_id: self.by_type_id,
        }
    }
}

impl Default for TopicRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::events::{
        BoardEvent, IncidentEvent, LlmEvent, MemoryEvent, MessageEvent, ObservabilityEvent,
        QuestionEvent, SessionEvent, SlotEvent, SystemEvent, TaskEvent, WorkerEvent,
    };
    use std::time::Duration;

    fn build_full_registry() -> TopicRegistry {
        TopicRegistryBuilder::new()
            .register::<SlotEvent>()
            .register::<BoardEvent>()
            .register::<TaskEvent>()
            .register::<QuestionEvent>()
            .register::<LlmEvent>()
            .register::<WorkerEvent>()
            .register::<MemoryEvent>()
            .register::<MessageEvent>()
            .register::<SessionEvent>()
            .register::<SystemEvent>()
            .register::<ObservabilityEvent>()
            .register::<IncidentEvent>()
            .build()
    }

    #[test]
    fn register_12_domains_covers_all() {
        let reg = build_full_registry();
        assert_eq!(reg.len(), 12);
        for d in Domain::ALL {
            assert!(reg.any_topic(d).is_some(), "missing {:?}", d);
        }
    }

    #[test]
    fn topic_lookup_by_type_returns_correct_domain() {
        let reg = build_full_registry();

        assert_eq!(reg.topic::<BoardEvent>().unwrap().domain(), Domain::Board);
        assert_eq!(reg.topic::<SlotEvent>().unwrap().domain(), Domain::Slot);
        assert_eq!(
            reg.topic::<IncidentEvent>().unwrap().domain(),
            Domain::Incident
        );
    }

    #[tokio::test]
    async fn typed_topic_fanout_json_deserializes_and_publishes() {
        let reg = build_full_registry();
        let board_topic = reg.topic::<BoardEvent>().unwrap();
        let mut rx = board_topic.subscribe();

        let any_board = reg.any_topic(Domain::Board).unwrap();
        let payload = serde_json::json!({
            "TaskCreated": {
                "task_id": "t-1",
                "title": "title",
                "category": "cat"
            }
        });
        any_board.fanout_json(&payload).expect("fanout ok");

        let got = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        match &*got {
            BoardEvent::TaskCreated {
                task_id,
                title,
                category,
            } => {
                assert_eq!(task_id, "t-1");
                assert_eq!(title, "title");
                assert_eq!(category, "cat");
            }
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn typed_topic_fanout_json_surfaces_bad_payload() {
        let reg = build_full_registry();
        let any_board = reg.any_topic(Domain::Board).unwrap();
        let bad = serde_json::json!({ "UnknownVariant": { "x": 1 } });
        let err = any_board.fanout_json(&bad).unwrap_err();
        match err {
            DispatchError::PayloadDeserialize { domain, .. } => {
                assert_eq!(domain, Domain::Board);
            }
            other => panic!("unexpected err {other:?}"),
        }
    }

    #[test]
    #[should_panic(expected = "already registered")]
    fn double_register_same_domain_panics() {
        let _ = TopicRegistryBuilder::new()
            .register::<BoardEvent>()
            .register::<BoardEvent>()
            .build();
    }

    #[test]
    fn registry_iter_visits_each_domain_once() {
        let reg = build_full_registry();
        let mut seen = Vec::new();
        for (d, _) in reg.iter() {
            seen.push(d);
        }
        seen.sort_by_key(|d| d.as_str());
        let mut expected: Vec<Domain> = Domain::ALL.to_vec();
        expected.sort_by_key(|d| d.as_str());
        assert_eq!(seen, expected);
    }
}
