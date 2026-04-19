//! `DomainEvent` trait — the contract every domain enum implements.
//!
//! Frozen lisp §4.2.a:
//!   fn domain() -> Domain
//!   fn kind(&self) -> &'static str
//!   fn payload_size_hint(&self) -> usize   // default 256
//!
//! The trait is intentionally narrow: storage + routing + claim-check only
//! need these three things. Anything richer (trace context, producer id,
//! ephemeral flag) lives in `AppendOpts`, not on the event itself.

use serde::{Serialize, de::DeserializeOwned};

use super::domain::Domain;

/// Common contract for every domain event enum.
///
/// `Send + Sync + 'static` + serde bounds come from the frozen lisp's
/// `:super` clause — they allow the dispatcher to store events in
/// `Arc<dyn Any>` fan-out channels while still being able to JSON-encode
/// for persistence.
pub trait DomainEvent:
    Send + Sync + Clone + Serialize + DeserializeOwned + std::fmt::Debug + 'static
{
    /// Static domain this enum belongs to. Compile-time constant — the
    /// dispatcher uses this to pick the right `Topic<T>` without inspecting
    /// any runtime field.
    fn domain() -> Domain
    where
        Self: Sized;

    /// Variant name for metrics/labels. Must be a stable string (snake_case
    /// preferred) and match 1:1 with enum variants.
    fn kind(&self) -> &'static str;

    /// Rough size estimate in bytes of the JSON-serialized payload. Used by
    /// `event-log` to decide claim-check (>8 KB goes to `blob_storage`).
    ///
    /// Default is 256 bytes, which covers small command-like variants.
    /// Variants with large text fields (prompt/response/summary) should
    /// override — e.g. `4096` for typical response previews and `16384`
    /// for full response text.
    fn payload_size_hint(&self) -> usize {
        256
    }
}
