//! 12 domain-event enums (see frozen lisp §4.2.a).
//!
//! Each submodule defines one `DomainEvent` implementer. `pub use` here
//! lets callers write `missiond_core::event::BoardEvent` instead of
//! `missiond_core::event::events::board::BoardEvent`.

pub mod board;
pub mod incident;
pub mod llm;
pub mod memory;
pub mod message;
pub mod observability;
pub mod question;
pub mod session;
pub mod slot;
pub mod system;
pub mod task;
pub mod worker;

pub use board::BoardEvent;
pub use incident::IncidentEvent;
pub use llm::{LlmEvent, Provider};
pub use memory::MemoryEvent;
pub use message::MessageEvent;
pub use observability::ObservabilityEvent;
pub use question::QuestionEvent;
pub use session::{SessionEndStatus, SessionEvent};
pub use slot::SlotEvent;
pub use system::SystemEvent;
pub use task::TaskEvent;
pub use worker::WorkerEvent;
