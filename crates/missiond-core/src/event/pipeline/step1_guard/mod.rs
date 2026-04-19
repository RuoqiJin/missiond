//! Pipeline step 1 · guard — causation + type resolution.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-1 guard:
//!
//! > 入口校验 — 排除循环事件 / 确认类型有效
//!
//! Two sub-components:
//!
//! * [`causation`] — reject appends whose `causation_depth` exceeds the
//!   frozen cap ([`causation::MAX_CAUSATION_DEPTH`] = 10). Frozen lisp §4.4
//!   cross-cutting: "每 append 的 causation_depth = 触发事件.depth + 1;
//!   超限 AppendError::CausalLoop".
//! * [`type_resolve`] — call `DomainEvent::domain()` / `kind()` /
//!   `payload_size_hint()` to produce the metadata that downstream steps
//!   (2 decide, 3 commit, 7 fanout) consume.
//!
//! Both guards run **synchronously** inside the producer's `append()` call
//! stack; their overhead is one `u8` comparison and three trait method
//! calls (all inlined at compile time). They are invoked from the append
//! hot path in [`crate::event::pipeline::step3_commit::log_writer`] and
//! [`crate::event::in_memory::InMemoryLog`].

pub mod causation;
pub mod type_resolve;

pub use causation::{check_causation, MAX_CAUSATION_DEPTH};
pub use type_resolve::ResolvedEventMeta;
